//! Browser glue: the functions `web/hero_web.js` registers in the WebAssembly import object.
//!
//! Keep this file and `web/hero_web.js` in sync. When the set of functions or their meaning
//! changes, bump [`HERO_WEB_VERSION`] here and `version` in the JS plugin together; the miniquad
//! loader then reports a mismatch in the console instead of failing in obscure ways.

use super::storage::{validate_key, KeyValueStore, StorageError};
use sapp_jsutils::JsObject;

/// Version of the `hero_web` JS plugin this build expects.
pub const HERO_WEB_VERSION: u32 = 4;

/// Prefix of every `localStorage` item written by the game.
const KEY_PREFIX: &str = "eiketsuden.";

// Imported from the `env` module that the miniquad loader fills from registered plugins.
#[link(wasm_import_module = "env")]
extern "C" {
    fn hero_web_ready();
    fn hero_web_panic(message: JsObject);
    fn hero_storage_has(key: JsObject) -> i32;
    fn hero_storage_get(key: JsObject) -> JsObject;
    fn hero_storage_set(key: JsObject, value: JsObject) -> i32;
    fn hero_storage_remove(key: JsObject) -> i32;
    fn hero_now_seconds() -> f64;
    fn hero_location_hash() -> JsObject;
    fn hero_pointer_inside() -> i32;
}

/// Checked by the miniquad JS loader against the plugin's `version` field.
#[no_mangle]
pub extern "C" fn hero_web_crate_version() -> u32 {
    HERO_WEB_VERSION
}

pub fn ready() {
    unsafe { hero_web_ready() }
}

/// Show a crash message over the canvas (the game cannot continue after a panic).
pub fn show_panic(message: &str) {
    unsafe { hero_web_panic(JsObject::string(message)) }
}

pub fn now_seconds() -> f64 {
    unsafe { hero_now_seconds() }
}

/// Whether the pointer is over the canvas and the page has focus.
pub fn pointer_inside() -> bool {
    unsafe { hero_pointer_inside() != 0 }
}

pub fn location_hash() -> String {
    let obj = unsafe { hero_location_hash() };
    let mut s = String::new();
    obj.to_string(&mut s);
    s
}

/// `localStorage`-backed [`KeyValueStore`].
pub struct LocalStorage;

fn full_key(key: &str) -> Result<String, StorageError> {
    validate_key(key)?;
    Ok(format!("{KEY_PREFIX}{key}"))
}

impl KeyValueStore for LocalStorage {
    fn get(&self, key: &str) -> Result<Option<String>, StorageError> {
        let k = full_key(key)?;
        if unsafe { hero_storage_has(JsObject::string(&k)) } == 0 {
            return Ok(None);
        }
        let obj = unsafe { hero_storage_get(JsObject::string(&k)) };
        let mut s = String::new();
        obj.to_string(&mut s);
        Ok(Some(s))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), StorageError> {
        let k = full_key(key)?;
        let ok = unsafe { hero_storage_set(JsObject::string(&k), JsObject::string(value)) };
        if ok == 1 {
            Ok(())
        } else {
            Err(StorageError {
                key: key.to_string(),
                msg: "browser storage is full or disabled".into(),
            })
        }
    }

    fn remove(&mut self, key: &str) -> Result<(), StorageError> {
        let k = full_key(key)?;
        if unsafe { hero_storage_remove(JsObject::string(&k)) } == 1 {
            Ok(())
        } else {
            Err(StorageError {
                key: key.to_string(),
                msg: "browser storage is disabled".into(),
            })
        }
    }

    fn location(&self) -> String {
        "browser localStorage".into()
    }
}
