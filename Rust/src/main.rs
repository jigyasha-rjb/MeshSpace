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
        .block(Block::default().title("💬 MeshSpace").borders(Borders::ALL))
        .wrap(Wrap { trim: true })
        .scroll((state.scroll_offset, 0));

    let input_widget = Paragraph::new(state.input.as_str())
        .block(Block::default().title("✍️ Message").borders(Borders::ALL));

    f.render_widget(chat_widget, chunks[0]);
    f.render_widget(input_widget, chunks[1]);
    if let Ok(cursor_x) = (chunks[1].x as usize + state.input.len() + 1).try_into() {
        f.set_cursor_position((cursor_x, chunks[1].y + 1));
    }
}

#[derive(Default)]
struct ChatState {
    messages: Vec<(String, String)>,
    input: String,
    users: HashMap<NodeId, String>,
    scroll_offset: u16,
}
use tokio::time::interval;

async fn chat_ui(
    mut receiver: GossipReceiver,
    sender: GossipSender,
    node_id: NodeId,
    display_name: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    print!("\x1B[2J\x1B[1;1H"); // Clear terminal
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let mut state = ChatState::default();
    let mut rebroadcast = interval(Duration::from_secs(5));
    // Send a "WhoIsThere" message so others reply with their AboutMe
    sender
        .broadcast(
            Message::new(MessageBody::WhoIsThere { from: node_id })
                .to_vec()
                .into(),
        )
        .await?;

    // Insert our own name into state immediately
    if let Some(name) = &display_name {
        state.users.insert(node_id, name.clone());
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
                            KeyCode::Esc => break,
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
                            KeyCode::Up => {
                            if state.scroll_offset > 0 {
                                state.scroll_offset -= 1;
                            }
                        }
                        KeyCode::Down => {
                            state.scroll_offset += 1;
                        }
                        KeyCode::PageUp => {
                            if state.scroll_offset >= 10 {
                                state.scroll_offset -= 10;
                            } else {
                                state.scroll_offset = 0;
                            }
                        }
                        KeyCode::PageDown => {
                            state.scroll_offset += 10;
                        }

                            _ => {}
                        }
                    }

                  Ok(Some(Event::Gossip(GossipEvent::Received(msg)))) = receiver.try_next() => {
            if let Ok(msg) = Message::from_bytes(&msg.content) {
                match msg.body {
                    MessageBody::WhoIsThere { from } => {
                        // Ignore our own WhoIsThere
                        if from != node_id {
                            if let Some(name) = &display_name {
                                let response = Message::new(MessageBody::AboutMe {
                                    from: node_id,
                                    name: name.clone(),
                                });
                                sender.broadcast(response.to_vec().into()).await?;
                            }
                        }
                    }
                    MessageBody::AboutMe { from, name } => {
                        if !state.users.contains_key(&from) {
                            state.messages.push(("System".into(), format!("{name} joined")));
                        }
                        state.users.insert(from, name.clone());
                    }

                    MessageBody::Message { from, text } => {
                        let name = state
                            .users
                            .get(&from)
                            .cloned()
                            .unwrap_or_else(|| from.fmt_short());

                        state.messages.push((name, text));

                    }
                }
            }
        }
                    _ = rebroadcast.tick() => {
                        if let Some(name) = &display_name {
                            let about = Message::new(MessageBody::AboutMe {
                                from: node_id,
                                name: name.clone(),
                            });
                            sender.broadcast(about.to_vec().into()).await?;
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

fn cli_header() {
    print!("\x1B[2J\x1B[1;1H");
    println!(
        r"
 __  __           _     ____                       
|  \/  | ___  ___| |__ / ___| _ __   __ _  ___ ___ 
| |\/| |/ _ \/ __| '_ \\___ \| '_ \ / _` |/ __/ _ \
| |  | |  __/\__ \ | | |___) | |_) | (_| | (_|  __/
|_|  |_|\___||___/_| |_|____/| .__/ \__,_|\___\___|
                             |_|                   
    "
    );
    // Updated welcome and credit messages with colors
    println!("\nWelcome to \x1B[33mMeshSpace\x1B[0m, your P2P Chat App!\n");
    println!("\x1B[34mBinding Nodes, Broadcasting Connections\x1B[0m\n");
    println!("Made by \x1B[32m@Pujan-DEV\x1B[0m && \x1B[32m@jigyasha-rjb\x1B[0m\n");
    println!("----------------------------------------------------\n");

    // How to use section
    println!("\x1B[36mHow to Use:\x1B[0m");

    println!("  - Start a room and share the \x1B[33mTicket\x1B[0m with others");
    println!("  - Or join a room using an existing \x1B[33mTicket\x1B[0m");
    println!("  - Press \x1B[33mEnter\x1B[0m to send a message");
    println!("  - Use \x1B[33m↑ / ↓\x1B[0m arrow keys to scroll messages");
    println!("  - Press \x1B[33mEsc\x1B[0m to exit the chat\n");
    println!("  - \x1B[2mPS: Chat does not auto-scroll — use arrows manually\x1B[0m\n");
    println!("----------------------------------------------------\n");
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut input = String::new();
    cli_header();
    io::stdout().flush()?;
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

    println!("\n");
    println!("\x1B[36mChoose an option:\x1B[0m"); // "Choose an option:" in cyan
    println!("1) Open a new chat room");
    println!("2) Join an existing chat room");

    io::stdout().flush()?;
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().to_string();
    input.clear();
    cli_header();

    let (topic, nodes) = if choice == "1" {
        let topic = TopicId::from_bytes(rand::random());
        println!("\x1B[34m> opening chat room for topic \x1B[0m {}\n", topic);
        (topic, vec![])
    } else if choice == "2" {
        print!("Enter ticket to join: ");
        io::stdout().flush()?;
        let mut ticket_input = String::new();
        io::stdin().read_line(&mut ticket_input)?;
        let ticket_str = ticket_input.trim();

        let Ticket { topic, nodes } = Ticket::from_str(ticket_str)?;
        println!(
            "\x1B[34m\n> joining chat room for topic \x1B[0m {}\n",
            topic
        );
        (topic, nodes)
    } else {
        println!("\x1B[31mInvalid choice\x1B[0m");
        return Ok(());
    };
    let endpoint = Endpoint::builder().discovery_n0().bind().await?;
    println!("\x1B[34m> our node id: \x1B[0m {}\n", endpoint.node_id());

    let gossip = Gossip::builder().spawn(endpoint.clone()).await?;

    let router = Router::builder(endpoint.clone())
        .accept(iroh_gossip::ALPN, gossip.clone())
        .spawn();

    let ticket = {
        let me = endpoint.node_addr().await?;
        let nodes = vec![me];
        Ticket { topic, nodes }
    };
    println!("\x1B[34m> ticket to join us: \x1B[0m{}\n", ticket);

    let node_ids = nodes.iter().map(|p| p.node_id).collect();
    if nodes.is_empty() {
        println!("\x1B[35m> waiting for nodes to join us...\x1B[0m\n");
    } else {
        println!(
            "\x1B[35m> trying to connect to {} nodes...\x1B[0m\n",
            nodes.len()
        );
        for node in nodes.into_iter() {
            endpoint.add_node_addr(node)?;
        }
    };

    let (sender, receiver) = gossip.subscribe_and_join(topic, node_ids).await?.split();
    println!("\x1B[35m> connected!\x1B[0m");

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
    WhoIsThere { from: NodeId },
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
