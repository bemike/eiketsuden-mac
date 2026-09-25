// Eiketsuden Reloaded — browser glue for the WebAssembly build.
// Registers a miniquad plugin that gives the game localStorage-backed save
// slots and lets it dismiss the HTML loading overlay once the first frame is up.
// Requires mq_js_bundle.js (sapp_jsutils helpers js_object/consume_js_object).
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
    // 1 when the key exists, 0 when it is missing or storage is unavailable.
    importObject.env.hero_storage_has = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      return s && s.getItem(k) !== null ? 1 : 0;
    };
    // Returns the stored string ("" when missing; check hero_storage_has first).
    importObject.env.hero_storage_get = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      var v = s ? s.getItem(k) : null;
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
    importObject.env.hero_storage_remove = function (key) {
      var k = consume_js_object(key);
      var s = storage();
      if (s) s.removeItem(k);
    };
  }

  miniquad_add_plugin({ register_plugin: register_plugin, name: "hero_web", version: 1 });
})();
