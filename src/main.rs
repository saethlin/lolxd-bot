use serde_derive::{Deserialize, Serialize};

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
    let args: Vec<_> = std::env::args().collect();
    let opt = Opt {
        logs: args[1].clone(),
        token: args[2].clone(),
        channel: args[3].clone(),
        botname: args[4].clone(),
    };

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
        let response = weeqwest::get(&format!(
            "https://slack.com/api/rtm.connect?token={}",
            opt.token
        ))
        .unwrap();

        let url = ::serde_json::from_slice::<ConnectResponse>(&response.bytes())
            .unwrap()
            .url;

        let mut websocket = weebsocket::Client::connect_secure(&url).unwrap();

        println!("Connected, ready to respond to messages");

        use weebsocket::Message::{Close, Ping, Pong, Text};

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

                            let request =
                                weeqwest::Request::post("https://slack.com/api/chat.postMessage")
                                    .unwrap()
                                    .add_header("authorization", &format!("Bearer {}", &opt.token))
                                    .json(
                                        serde_json::to_string(&Response {
                                            channel: &opt.channel,
                                            text: &text,
                                            username: &opt.botname,
                                        })
                                        .unwrap(),
                                    );
                            let response = weeqwest::send(&request).unwrap();
                            println!("{:?}", std::str::from_utf8(response.bytes()));
                        }
                    }
                }
                Ping(m) => websocket.send_message(&Pong(m)).unwrap(),
                Close => break,
                _ => {}
            }
        }

        println!("Reconnecting\n");
    }
}
