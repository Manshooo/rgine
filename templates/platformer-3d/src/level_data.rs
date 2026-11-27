use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Kind {
    Start,
    Platform,
    Coin,
    Checkpoint,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Piece {
    pub kind: Kind,
    pub position: [f32; 3],
    pub size: [f32; 3],
}
