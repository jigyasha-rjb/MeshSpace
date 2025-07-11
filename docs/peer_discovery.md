## 🧩 Peer Discovery and Identity Handshake (TUI Networking Logic)

Our Rust-based P2P chat app, built with `iroh`, `iroh-gossip`, and `ratatui`, implements a **lightweight and reliable identity handshake protocol**. This ensures every participant knows who’s connected in the chat room — **without flooding the network with redundant messages**.

---

### 🔄 The Challenge with Basic Gossip Systems

- Only peers who join early broadcast their usernames (`AboutMe`).
- New peers joining later don’t automatically learn who is already connected.
- Some naive solutions rebroadcast `AboutMe` messages frequently, causing spam and inefficient network traffic.

---

### ✅ Our Solution: Two-Phase Identity Handshake

We designed a clean two-step handshake that:

1. **New users query who’s already in the room.**
2. **Existing users respond with their identities.**

This keeps the network traffic minimal and the UI updates precise.

---

### 🗣 Step 1: `WhoIsThere` — New User Announces Presence

When a new user joins, they broadcast a **`WhoIsThere`** message to ask:

> “Who else is in this chat room right now?”

```rust
MessageBody::WhoIsThere { from: NodeId }
```

This acts as a discovery request.

---

### 🧾 Step 2: `AboutMe` — Existing Users Introduce Themselves

Upon receiving `WhoIsThere`, every existing peer replies with an **`AboutMe`** message containing their username:

```rust
MessageBody::AboutMe { from: NodeId, name: String }
```

Each `AboutMe` message:

- Is sent **only once** per existing user in response to a new user’s query.
- Triggers a `"System: {name} joined"` notification **only if this user is new to the recipient**.
- Is stored in the local `ChatState.users` map keyed by `NodeId`.

---

### ✅ Summary of Behavior

| Event                       | Who Receives It?          | Outcome                                                   |
| --------------------------- | ------------------------- | --------------------------------------------------------- |
| New user joins              | All existing users        | They receive `WhoIsThere` and reply with their `AboutMe`. |
| Existing users see new user | All existing users        | See a single `"System: {name} joined"` message **once**.  |
| New user sees all existing  | New user                  | Receives all `AboutMe` messages with usernames.           |
| Redundant broadcasts        | No one (filtered locally) | Prevents duplicate join messages and repeated broadcasts. |

---

### 🧪 Message Enum Overview

```rust
enum MessageBody {
    WhoIsThere { from: NodeId },              // Sent by the new joiner to query peers
    AboutMe { from: NodeId, name: String },   // Sent by existing peers to introduce themselves
    Message { from: NodeId, text: String },   // Regular chat messages
}
```

---

### ✅ Benefits of This Approach

- 🔇 **No noisy repeated broadcasts** — each identity announcement happens once per peer.
- 💡 **Efficient real-time user discovery** when someone new joins.
- 🧠 **Stateful tracking of connected peers**, enabling accurate display of who is in the chat.
- 🧼 **Clean user experience** with proper join notifications and minimal network chatter.
