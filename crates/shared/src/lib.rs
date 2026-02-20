use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
    pub user_id: Uuid,
    pub lon: f64,
    pub lat: f64,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub room_id: String,
    pub from_user: Uuid,
    pub text: String,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteEvent {
    pub from_user: Uuid,
    pub to_user: Uuid,
    pub mode: String,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RealtimePacket {
    Position(PositionUpdate),
    Chat(ChatMessage),
    Invite(InviteEvent),
    Heartbeat,
}
