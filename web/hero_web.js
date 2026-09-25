// Eiketsuden Reloaded — browser glue for the WebAssembly build.
// Registers a miniquad plugin that gives the game localStorage-backed save
// slots, the wall clock, the URL hash (launch options such as #gallery), lets
// it dismiss the HTML loading overlay once the first frame is up and show a
// crash message if it panics.
// Requires mq_js_bundle.js (sapp_jsutils helpers js_object/consume_js_object).
//
// Keep in sync with crates/hero-game/src/platform/web.rs. Bump `version`
// below and HERO_WEB_VERSION there together whenever the API changes.
"use strict";
(function () {
  function storage() {
    try {
      return window.localStorage;
    } catch (e) {
      return null; // storage disabled (private mode, sandboxed iframe, ...)
    }
  }

  function register_plugin(importObject) {
    importObject.env.hero_web_ready = function () {
      var el = document.getElementById("loading");
      if (el) el.remove();
      var canvas = document.getElementById("glcanvas");
      if (canvas) canvas.focus();
    };
    // The game panicked and cannot continue: show the message over the frozen canvas.
    importObject.env.hero_web_panic = function (message) {
      var text = consume_js_object(message);
      var box = document.getElementById("crash");
      if (!box) {
        box = document.createElement("div");
        box.id = "crash";
        box.setAttribute("role", "alert");
        document.body.appendChild(box);
      }
      box.textContent = "";
      var title = document.createElement("h1");
      title.textContent = "오류가 발생했습니다 / The game crashed";
      var hint = document.createElement("p");
      hint.textContent = "페이지를 새로 고치면 다시 시작합니다. 버그 신고 시 아래 내용을 함께 보내 주세요.";
      var detail = document.createElement("pre");
      detail.textContent = text;
      box.appendChild(title);
      box.appendChild(hint);
      box.appendChild(detail);
    };
    // 1 when the key exists, 0 when it is missing or storage is unavailable.
    importObject.env.hero_storage_has = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      try {
        return s && s.getItem(k) !== null ? 1 : 0;
      } catch (e) {
        console.error("hero_storage_has failed", e);
        return 0;
      }
    };
    // Returns the stored string ("" when missing; check hero_storage_has first).
    importObject.env.hero_storage_get = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      var v = null;
      try {
        v = s ? s.getItem(k) : null;
      } catch (e) {
        console.error("hero_storage_get failed", e);
      }
      return js_object(v === null ? "" : v);
    };
    // Returns 1 on success, 0 when the write failed (quota exceeded / storage disabled).
    importObject.env.hero_storage_set = function (key, value) {
      var k = consume_js_object(key);
      var v = consume_js_object(value);
      var s = storage();
      if (!s) return 0;
      try {
        s.setItem(k, v);
        return 1;
      } catch (e) {
        console.error("hero_storage_set failed", e);
        return 0;
      }
    };
    // Returns 1 on success (also when the key did not exist), 0 when storage is unavailable.
    importObject.env.hero_storage_remove = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      if (!s) return 0;
      try {
        s.removeItem(k);
        return 1;
      } catch (e) {
        console.error("hero_storage_remove failed", e);
        return 0;
      }
    };
    // Wall clock in Unix seconds (fractional), for save timestamps.
    importObject.env.hero_now_seconds = function () {
      return Date.now() / 1000;
    };
    // The URL fragment including '#', e.g. "#gallery" ("" when there is none).
    importObject.env.hero_location_hash = function () {
      return js_object(window.location.hash || "");
    };
  }

  miniquad_add_plugin({ register_plugin: register_plugin, name: "hero_web", version: 3 });
})();
