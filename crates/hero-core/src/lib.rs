//! Platform-independent core of Eiketsuden Reloaded: data schema, pack loading, drama
//! scripting, campaign state and the battle rules. No graphics, audio or OS access
//! (except the optional [`pack::DirSource`]), so it runs natively, in WebAssembly and in tests.

pub mod battle;
pub mod battledef;
pub mod campaign;
pub mod data;
pub mod drama;
pub mod geom;
pub mod guide;
pub mod inventory;
pub mod map;
pub mod media_index;
pub mod pack;
pub mod rng;
pub mod save;
pub mod script;
