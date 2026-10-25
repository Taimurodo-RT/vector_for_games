// Installed into PixaTool's page: answers its file dialogs and clicks its buttons, so another
// program can drive it. Nothing in PixaTool's own files changes.
(() => {
  if (window.__motif) return "ready";
  const rt = cr_getC2Runtime();
  const m = { answers: [], loaded: 0, presets: 0 };
  window.__motif = m;
  const g = Function("return this")();
  // Image and preset pickers.
  // PixaTool defines its own on start of layout, so ours must survive being assigned over.
  const open = (type, multi) => {
    const url = m.answers.shift();
    setTimeout(() => c2_callFunction(multi ? "GetMultipleFilesFromDialog" : "GetFileFromDialog", [url]), 0);
  };
  Object.defineProperty(g, "openFileDialog", { get: () => open, set: () => {}, configurable: true });
  // NW.js save and folder dialogs: the answer is read from the input's value on change.
  for (const id of ["c2nwSaveDialog", "c2nwChooseFolderDialog", "c2nwOpenFileDialog"]) {
    const el = document.getElementById(id);
    Object.defineProperty(el, "value", { get: () => m.answers[0] || "", configurable: true });
    el.click = () => setTimeout(() => { el.dispatchEvent(new Event("change")); m.answers.shift(); }, 0);
  }
  // Count finished loads: a picture in the preview, a preset file read.
  const trigger = rt.trigger.bind(rt);
  rt.trigger = (method, inst, value) => {
    if (method === cr.plugins_.Sprite.prototype.cnds.OnURLLoaded) m.loaded++;
    if (method === cr.plugins_.AJAX.prototype.cnds.OnComplete) m.presets++;
    return trigger(method, inst, value);
  };
  // Top menu buttons by their name; Import is the one without a name.
  m.button = (name) => {
    for (const t of rt.types_by_index) for (const inst of t.instances) {
      const iv = inst.instance_vars || [];
      if (iv[0] === "button" && iv.includes("TopMenu") && iv[4] === name) {
        const cx = inst.x + inst.width / 2, cy = inst.y + inst.height / 2;
        const px = inst.layer.layerToCanvas(cx, cy, true), py = inst.layer.layerToCanvas(cx, cy, false);
        const r = rt.canvas.getBoundingClientRect();
        return [r.left + px * r.width / rt.canvas.width, r.top + py * r.height / rt.canvas.height];
      }
    }
    return null;
  };
  m.click = (name) => {
    const at = m.button(name);
    if (!at) return false;
    const [x, y] = at;
    const opts = { bubbles: true, cancelable: true, clientX: x, clientY: y, pageX: x, pageY: y, button: 0, buttons: 1, pointerId: 1, pointerType: "mouse", isPrimary: true };
    const c = rt.canvas;
    c.dispatchEvent(new PointerEvent("pointermove", opts));
    document.dispatchEvent(new MouseEvent("mousemove", opts));
    c.dispatchEvent(new PointerEvent("pointerdown", opts));
    c.dispatchEvent(new MouseEvent("mousedown", opts));
    setTimeout(() => {
      const up = { ...opts, buttons: 0 };
      c.dispatchEvent(new PointerEvent("pointerup", up));
      c.dispatchEvent(new MouseEvent("mouseup", up));
      c.dispatchEvent(new MouseEvent("click", up));
    }, 50);
    return true;
  };
  return "installed";
})()
