// 注入每个插件上下文的 `sayso` API。原生函数只收发 JSON 文本，结构转换都在这里完成。
(() => {
  const native = globalThis.__sayso;
  delete globalThis.__sayso;
  const emit = (op, fields) => native.emit(JSON.stringify({ op, ...fields }));
  const languages = JSON.parse(native.languages);

  globalThis.sayso = Object.freeze({
    /** 本次的目标语言与本地语言：`{ target: { code, name }, native: { code, name } }`。 */
    languages,
    blocks: Object.freeze({
      upsert: (block) => emit("block.upsert", { block }),
      remove: (id) => emit("block.remove", { id }),
    }),
    store: Object.freeze({
      put: (collection, key, record) => emit("store.put", { collection, key, record }),
      patch: (collection, key, { set = {}, increment = {} } = {}) => emit("store.patch", { collection, key, set, increment }),
      delete: (collection, key) => emit("store.delete", { collection, key }),
      list: async (collection) => JSON.parse(native.records(collection)),
    }),
    speak: (text, language) => emit("speak", { text, language }),
    publish: (event) => emit("publish", { event }),
    llm: (system, user) => native.llm(system, user),
    insert: (text) => native.insert(text),
    text: Object.freeze({
      /** 文本是否可能夹杂本地语言；纯目标语言的文本可以跳过模型调用。 */
      mayContainNative: (text) => native.mayContainNative(text),
    }),
    fs: Object.freeze({
      readText: (path) => native.fs("read", path, ""),
      writeText: (path, text) => { native.fs("write", path, text); },
      appendText: (path, text) => { native.fs("append", path, text); },
      exists: (path) => native.fs("exists", path, "") === "true",
      mkdir: (path) => { native.fs("mkdir", path, ""); },
    }),
  });

  const log = (...values) => native.log(values.map((value) => (typeof value === "string" ? value : JSON.stringify(value))).join(" "));
  globalThis.console = Object.freeze({ log, info: log, warn: log, error: log });
})();
