extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate markov;
extern crate reqwest;
extern crate serde_json;
extern crate structopt;
extern crate websocket;

use structopt::StructOpt;
use websocket::message::OwnedMessage;

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

    let mut sentences = Vec::new();

    // Load message history
    for file_path in std::fs::read_dir(&opt.logs)?
        .map(|dir_entry| dir_entry.unwrap())
        .filter(|dir_entry| dir_entry.file_type().unwrap().is_dir())
        .flat_map(|directory| std::fs::read_dir(directory.path()).unwrap())
        .map(|file| file.unwrap().path())
        .filter(|file_path| file_path.to_str().unwrap().ends_with("json"))
    {
        let contents = std::fs::read_to_string(&file_path)?;
        let messages: Vec<Message> = serde_json::from_str(&contents).unwrap();
        for text in messages.into_iter().filter_map(|message| message.text) {
            if text.split_whitespace().count() > 1 {
                sentences.push(text);
            }
        }
    }

    let mut chain = markov::Chain::of_order(2);

    for sentence in sentences.iter() {
        chain.feed(sentence.split_whitespace().collect());
    }

    println!("Message history loaded");

    loop {
        let client = reqwest::Client::new();

        let resp = client
            .get(&format!(
                "https://slack.com/api/rtm.connect?token={}",
                opt.token
            )).send()
            .unwrap()
            .text()
            .unwrap();

        let url: ConnectResponse = ::serde_json::from_str(&resp).unwrap();
        let url = url.url;

        let mut websocket = websocket::ClientBuilder::new(&url)
            .unwrap()
            .connect_secure(None)
            .unwrap();

        println!("Connected, ready to respond to messages");

        while let Ok(OwnedMessage::Text(m)) = websocket.recv_message() {
            println!("{}\n", m);
            if let Ok(parsed) = ::serde_json::from_str::<WsMessage>(&m) {
                // Only respond to normal messages sent in the specified channel
                if &parsed.ty == "message" && parsed.channel == opt.channel {
                    // Way too many one-word messages come out otherwise
                    let text = loop {
                        let attempt = chain.generate();
                        println!("Attempt:\n{:?}", attempt);
                        if attempt.len() > 5 {
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
                        }).send()
                        .unwrap();
                }
            }
        }

        println!("Reconnecting\n");
    }
}
