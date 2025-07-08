use std::{collections::HashMap, fmt, str::FromStr};

use anyhow::Result;
use clap::Parser;
use futures_lite::StreamExt;
use iroh::{Endpoint, NodeAddr, NodeId, protocol::Router};
use iroh_gossip::net::GossipSender;
use iroh_gossip::{
    net::{Event, Gossip, GossipEvent, GossipReceiver},
    proto::TopicId,
};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
#[derive(Parser, Debug)]
struct Args {
    #[clap(short, long)]
    name: Option<String>,
    #[clap(short, long, default_value = "0")]
    bind_port: u16,
    #[clap(subcommand)]
    command: Command,
}
use std::time::Duration;
use tokio::time::sleep;

// use color_eyre::Result;
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode},
};

use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    text::Text,
    widgets::{Block, Borders, Paragraph, Wrap},
};
fn render_ui(f: &mut Frame, state: &ChatState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(f.area());

    let chat = state
        .messages
        .iter()
        .map(|(u, m)| format!("{u}: {m}"))
        .collect::<Vec<_>>()
        .join("\n");

    let chat_widget = Paragraph::new(Text::from(chat))
        .block(Block::default().title("💬 Chat").borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    let input_widget = Paragraph::new(state.input.as_str())
        .block(Block::default().title("✍️ Message").borders(Borders::ALL));

    f.render_widget(chat_widget, chunks[0]);
    f.render_widget(input_widget, chunks[1]);
    f.set_cursor(chunks[1].x + state.input.len() as u16 + 1, chunks[1].y + 1);
}

#[derive(Default)]
struct ChatState {
    messages: Vec<(String, String)>,
    input: String,
    users: HashMap<NodeId, String>,
}

async fn chat_ui(
    mut receiver: GossipReceiver,
    sender: GossipSender,
    node_id: NodeId,
    display_name: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut state = ChatState::default();

    // Announce your presence if you have a name
    if let Some(name) = &display_name {
        state.users.insert(node_id, name.clone());
        let message = Message::new(MessageBody::AboutMe {
            from: node_id,
            name: name.clone(),
        });
        sender.broadcast(message.to_vec().into()).await?;
    }

    // Spawn a thread to read terminal input and send it through channel
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel(1);
    std::thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(50)).unwrap() {
                if let Ok(CEvent::Key(key)) = event::read() {
                    let _ = input_tx.blocking_send(key);
                }
            }
        }
    });

    loop {
        terminal.draw(|f| render_ui(f, &state))?;

        tokio::select! {
            Some(key) = input_rx.recv() => {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Enter => {
                        let msg = state.input.trim().to_string();
                        if !msg.is_empty() {
                            let outgoing = Message::new(MessageBody::Message {
                                from: node_id,
                                text: msg.clone(),
                            });
                            sender.broadcast(outgoing.to_vec().into()).await?;
                            let label = display_name.clone().unwrap_or_else(|| "You".into());
                            state.messages.push((label, msg));
                            state.input.clear();
                        }
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Char(c) => {
                        state.input.push(c);
                    }
                    _ => {}
                }
            }

            Ok(Some(Event::Gossip(GossipEvent::Received(msg)))) = receiver.try_next() => {
                if let Ok(msg) = Message::from_bytes(&msg.content) {
                    match msg.body {
                        MessageBody::AboutMe { from, name } => {
                            state.users.insert(from, name.clone());
                            state.messages.push(("System".into(), format!("{name} joined")));
                        }
                        MessageBody::Message { from, text } => {
                            let name = state.users.get(&from).cloned().unwrap_or_else(|| from.fmt_short());
                            state.messages.push((name, text));
                        }
                    }
                }
            }

            _ = sleep(Duration::from_millis(100)) => {}
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

#[derive(Parser, Debug)]
enum Command {
    Open,
    Join { ticket: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut input = String::new();

    print!("Enter your name (optional): ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut input)?;
    let name = input.trim();
    let name = if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    };
    input.clear();

    print!("Enter port to bind (default 0): ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut input)?;
    let bind_port = input.trim().parse::<u16>().unwrap_or(0);
    print!("{}", bind_port);
    input.clear();

    println!("Choose an option:");
    println!("1) Open a new chat room");
    println!("2) Join an existing chat room");
    print!("Enter choice (1 or 2): ");

    io::stdout().flush()?;
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().to_string();
    input.clear();
    let (topic, nodes) = if choice == "1" {
        let topic = TopicId::from_bytes(rand::random());
        println!("> opening chat room for topic {topic}");
        (topic, vec![])
    } else if choice == "2" {
        print!("Enter ticket to join: ");
        io::stdout().flush()?;
        let mut ticket_input = String::new();
        io::stdin().read_line(&mut ticket_input)?;
        let ticket_str = ticket_input.trim();

        let Ticket { topic, nodes } = Ticket::from_str(ticket_str)?;
        println!("> joining chat room for topic {topic}");
        (topic, nodes)
    } else {
        println!("Invalid choice");
        return Ok(());
    };
    let endpoint = Endpoint::builder().discovery_n0().bind().await?;
    println!("> our node id: {}", endpoint.node_id());

    let gossip = Gossip::builder().spawn(endpoint.clone()).await?;

    let router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let ticket = {
        let me = endpoint.node_addr().await?;
        let nodes = vec![me];
        Ticket { topic, nodes }
    };
    println!("> ticket to join us: {ticket}");

    let node_ids = nodes.iter().map(|p| p.node_id).collect();
    if nodes.is_empty() {
        println!("> waiting for nodes to join us...");
    } else {
        println!("> trying to connect to {} nodes...", nodes.len());
        for node in nodes.into_iter() {
            endpoint.add_node_addr(node)?;
        }
    };

    let (sender, receiver) = gossip.subscribe_and_join(topic, node_ids).await?.split();
    println!("> connected!");

    chat_ui(receiver, sender, endpoint.node_id(), name).await?;
    router.shutdown().await?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    body: MessageBody,
    nonce: [u8; 16],
}

#[derive(Debug, Serialize, Deserialize)]
enum MessageBody {
    AboutMe { from: NodeId, name: String },
    Message { from: NodeId, text: String },
}

impl Message {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(Into::into)
    }

    pub fn new(body: MessageBody) -> Self {
        Self {
            body,
            nonce: rand::random(),
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serde_json::to_vec is infallible")
    }
}

// Handle incoming events
async fn subscribe_loop(mut receiver: GossipReceiver) -> Result<()> {
    let mut names = HashMap::new();
    while let Some(event) = receiver.try_next().await? {
        if let Event::Gossip(GossipEvent::Received(msg)) = event {
            match Message::from_bytes(&msg.content)?.body {
                MessageBody::AboutMe { from, name } => {
                    names.insert(from, name.clone());
                    println!("> {} is now known as {}", from.fmt_short(), name);
                }
                MessageBody::Message { from, text } => {
                    let name = names
                        .get(&from)
                        .map_or_else(|| from.fmt_short(), String::to_string);
                    println!("{}: {}", name, text);
                }
            }
        }
    }
    Ok(())
}

fn input_loop(line_tx: tokio::sync::mpsc::Sender<String>) -> Result<()> {
    let mut buffer = String::new();
    let stdin = std::io::stdin(); // We get `Stdin` here.
    loop {
        stdin.read_line(&mut buffer)?;
        line_tx.blocking_send(buffer.clone())?;
        buffer.clear();
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Ticket {
    topic: TopicId,
    nodes: Vec<NodeAddr>,
}

impl Ticket {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes).map_err(Into::into)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serde_json::to_vec is infallible")
    }
}

impl fmt::Display for Ticket {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut text = data_encoding::BASE32_NOPAD.encode(&self.to_bytes()[..]);
        text.make_ascii_lowercase();
        write!(f, "{}", text)
    }
}

impl FromStr for Ticket {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = data_encoding::BASE32_NOPAD.decode(s.to_ascii_uppercase().as_bytes())?;
        Self::from_bytes(&bytes)
    }
}

