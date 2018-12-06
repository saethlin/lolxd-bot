use serde_derive::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(StructOpt)]
struct Opt {
    logs: String,
    token: String,
    channel: String,
    botname: String,
}

#[derive(Deserialize)]
struct Message {
    text: Option<String>,
}

#[derive(Deserialize)]
struct ConnectResponse {
    url: String,
}

#[derive(Deserialize)]
struct WsMessage {
    #[serde(rename = "type")]
    ty: String,
    channel: String,
    #[serde(rename = "user")]
    _user: String,
}

#[derive(Serialize)]
struct Response<'a> {
    channel: &'a str,
    text: &'a str,
    username: &'a str,
}

fn main() -> Result<(), std::io::Error> {
    let opt = Opt::from_args();

    let rx = {
        let (tx, rx) = std::sync::mpsc::channel();

        // Load message history
        for file_path in std::fs::read_dir(&opt.logs)?
            .map(|dir_entry| dir_entry.unwrap())
            .filter(|dir_entry| dir_entry.file_type().unwrap().is_dir())
            .flat_map(|directory| std::fs::read_dir(directory.path()).unwrap())
            .map(|file| file.unwrap().path())
            .filter(|file_path| file_path.to_str().unwrap().ends_with("json"))
        {
            let sender = tx.clone();
            std::thread::spawn(move || {
                let mut sentences = Vec::new();
                let contents = std::fs::read_to_string(&file_path).unwrap();
                let messages: Vec<Message> = serde_json::from_str(&contents).unwrap();
                for text in messages.into_iter().filter_map(|message| message.text) {
                    if text.split_whitespace().count() > 1 {
                        sentences.push(text);
                    }
                }
                sender.send(sentences).unwrap();
            });
        }
        rx
    };

    let mut chain = markov::Chain::of_order(2);

    for sentence in rx.iter().flat_map(|day| day.into_iter()) {
        chain.feed_str(&sentence);
    }

    println!("Message history loaded");

    loop {
        let client = reqwest::Client::new();

        let json = client
            .get(&format!(
                "https://slack.com/api/rtm.connect?token={}",
                opt.token
            ))
            .send()
            .and_then(|mut r| r.text())
            .unwrap();

        let url = ::serde_json::from_str::<ConnectResponse>(&json)
            .unwrap()
            .url;

        let mut websocket = websocket::ClientBuilder::new(&url)
            .unwrap()
            .connect_secure(None)
            .unwrap();

        println!("Connected, ready to respond to messages");

        use websocket::OwnedMessage::{Close, Ping, Pong, Text};

        while let Ok(message) = websocket.recv_message() {
            match message {
                Text(m) => {
                    println!("{}\n", m);
                    if let Ok(parsed) = ::serde_json::from_str::<WsMessage>(&m) {
                        // Only respond to normal messages sent in the specified channel
                        if &parsed.ty == "message" && parsed.channel == opt.channel {
                            // Way too many one-word messages come out otherwise
                            let text = loop {
                                let attempt = chain.generate();
                                println!("Attempt:\n{:?}", attempt);
                                if attempt.len() > 1 {
                                    break attempt.join(" ");
                                }
                            };

                            println!("Responding:\n{}\n\n", text);

                            client
                                .post("https://slack.com/api/chat.postMessage")
                                .bearer_auth(&opt.token)
                                .json(&Response {
                                    channel: &opt.channel,
                                    text: &text,
                                    username: &opt.botname,
                                })
                                .send()
                                .unwrap();
                        }
                    }
                }
                Ping(m) => websocket.send_message(&Pong(m)).unwrap(),
                Close(_) => break,
                _ => {}
            }
        }

        println!("Reconnecting\n");
    }
}
