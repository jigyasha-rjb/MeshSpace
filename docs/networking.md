# 🧠 Networking Behind the Scenes: Powered by `iroh` + `iroh-gossip`

  

This document outlines the networking logic that powers the peer-to-peer chat application. All message transmission, identity sharing, and inter-peer communication are driven through a structured networking process implemented using `iroh` and `iroh-gossip`.

  

📌 _This file focuses **only** on the networking layer. A separate `tui.md` describes the terminal user interface (TUI) layer and `peer_discovery.md` the explains the nodes discovery process.

  

---

  

## 🧩 Core Networking Crates

  

This system relies on two primary crates:

- **`iroh`**: Establishes secure, addressable communication between nodes.

- **`iroh-gossip`**: Implements the gossip-based pubsub layer to propagate messages across all peers.

  

In this architecture, `iroh` provides the underlying transport layer (the **networking road**), and `iroh-gossip` acts as the **message delivery mechanism**, enabling real-time sync across distributed nodes.

  

---

  

## ⚙️ Functional Breakdown of Networking Flow

  

### 1️⃣ Network Initialization — `Endpoint`

  

The networking stack begins by initializing an `Endpoint`. This establishes a unique node identity, binds a local port, and optionally enables discovery.

  

```rust

let endpoint = Endpoint::builder().discovery_n0().bind().await?;

```

  

- `discovery_n0()` enables peer discovery over local networks.

- `bind()` opens a port for communication.
  

---

  

### 2️⃣ Session Bootstrapping: Open or Join

  

Peers either open a new session or join an existing one via an encoded `Ticket`.

  

#### Open a Session

```rust

let topic = TopicId::from_bytes(rand::random());

let me = endpoint.node_addr().await?;

let ticket = Ticket { topic, nodes: vec![me] };

```

A unique `topic` is created and bundled with the local node address. The resulting ticket can be shared with others.

  

#### Join a Session

```rust

let Ticket { topic, nodes } = Ticket::from_str(ticket_str)?;

endpoint.add_node_addr(node)?;

```

The ticket is decoded to retrieve topic and node information, and a connection to the remote peer is established.

  

  

---

  

### 3️⃣ Enabling Gossip Protocol — `Gossip::spawn()`

  

The gossip layer enables a publish-subscribe pattern. Once the `Gossip` instance is spawned, all peers subscribing to the same topic can receive broadcasts from one another.

  

```rust

let gossip = Gossip::builder().spawn(endpoint.clone()).await?;

```

  

To enable message flow, the peer subscribes to the topic and obtains a `sender/receiver` split:

  

```rust

let (sender, receiver) = gossip.subscribe_and_join(topic, node_ids).await?.split();

```


  
<p align="center">
  <img src="../assets/sender-receiver-flow.png" alt="Broadcast Flow" width="400">
  <br/>
</p>

> Figure: Broadcast message flow across peers using gossip

 

---

  

### 4️⃣ Tickets — Session Invitations

  

A `Ticket` is an encoded string that includes the gossip topic and a list of peer node addresses.

  

```rust

impl fmt::Display for Ticket {

    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {

        let text = data_encoding::BASE32_NOPAD.encode(&self.to_bytes());

        write!(f, "{}", text.to_ascii_lowercase())

    }

}

```

  

This string acts as a portable invitation for session joining.


```txt
pmwhi33qnfvqsow3gibt...<snipped>...tois2pvmx2
```

> Example of base32-encoded ticket string in terminal output


---

  

### 5️⃣ Message Protocol — `Message` and `MessageBody`

  

All communication between peers is encapsulated in the `Message` struct, which includes metadata and payload via the `MessageBody` enum.

  

```rust

enum MessageBody {

    AboutMe { from: NodeId, name: String },

    Message { from: NodeId, text: String },

}

```

  

Each message is serialized into JSON, and then broadcast using the `sender`.

  

```rust

let message = Message::new(MessageBody::Message { from: node_id, text });

sender.broadcast(message.to_vec().into()).await?;

```

  

<p align="center">
  <img src="../assets/serialization-to-reception-flow.png" alt="Serialization to Reception Flow" width="300">
  <br/>
</p>
> Figure: End-to-End Message Path in iroh-gossip: Serialize → Gossip → Receive → Handle


---

  

### 6️⃣ Name Announcement — `AboutMe` Messaging

  

Upon connection, each peer announces its identity by broadcasting an `AboutMe` message. This ensures that each `NodeId` can be mapped to a human-readable name.

  

```rust

let message = Message::new(MessageBody::AboutMe {

    from: node_id,

    name: my_name,

});

sender.broadcast(message.to_vec().into()).await?;

```

  

---

  

### 7️⃣ Multi-Peer Mesh — Gossip Propagation Topology

  

The system implements a **full-mesh** gossip network. Each peer communicates with all others by default, ensuring uniform message propagation.

  
<p align="center">
  <img src="../assets/full-mesh-flow.png" alt="Full Mesh Flow" width="1200">
  <br/>
</p>

> Figure: Enhanced flowchart showing full-mesh connectivity



  
No central node exists. All participants are equal in authority and visibility.


---

  

### 8️⃣ Graceful Shutdown — `router.shutdown()`

  

The router instance is terminated cleanly using:

  

```rust

router.shutdown().await?;

```

  

This avoids dangling gossip streams and ensures that node resources are properly released on exit.

  

  

---

  


## ✅ Conclusion

  

The system follows a clear peer-to-peer model. Each peer initializes a communication channel, optionally invites others via a ticket, and enters a topic-based gossip network. Messages are serialized, broadcasted, and processed uniformly across all participating peers.

  

No centralized server is required. Connectivity and communication are handled entirely through self-contained Rust code.

  

  

---

  

