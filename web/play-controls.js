// Keep independent fingers, keys and controllers from releasing each other.
export class ControllerInput {
  constructor(onChange) { this.sources = new Map(); this.buttons = 0; this.onChange = onChange; }
  set(source, buttons) {
    if (buttons) this.sources.set(source, buttons);
    else this.sources.delete(source);
    const combined = [...this.sources.values()].reduce((mask, value) => mask | value, 0);
    if (combined === this.buttons) return;
    const previous = this.buttons;
    this.buttons = combined;
    this.onChange(combined, previous);
  }
  clear() { this.sources.clear(); this.set("reset", 0); }
}

export function readGamepads(pads, buttons) {
  let mask = 0;
  let connected = false;
  for (const pad of pads) {
    if (!pad?.connected || pad.mapping !== "standard") continue;
    connected = true;
    const down = index => pad.buttons[index]?.pressed;
    if (down(0)) mask |= buttons.a;
    if (down(1) || down(2)) mask |= buttons.b;
    if (down(8)) mask |= buttons.select;
    if (down(9)) mask |= buttons.start;
    if (down(12) || pad.axes[1] < -0.4) mask |= buttons.up;
    if (down(13) || pad.axes[1] > 0.4) mask |= buttons.down;
    if (down(14) || pad.axes[0] < -0.4) mask |= buttons.left;
    if (down(15) || pad.axes[0] > 0.4) mask |= buttons.right;
  }
  return { buttons: mask, connected };
}

export function bindTouchControls(root, input, buttons) {
  const pointers = new Map();
  const dpad = root.querySelector(".dpad-hitbox");
  const update = event => {
    const control = pointers.get(event.pointerId);
    if (!control) return;
    let mask = buttons[control.dataset.button] || 0;
    if (control === dpad) {
      const rect = dpad.getBoundingClientRect();
      const x = (event.clientX - rect.left) / rect.width - 0.5;
      const y = (event.clientY - rect.top) / rect.height - 0.5;
      mask = 0;
      if (x < -0.16) mask |= buttons.left;
      if (x > 0.16) mask |= buttons.right;
      if (y < -0.16) mask |= buttons.up;
      if (y > 0.16) mask |= buttons.down;
    }
    input.set(`pointer-${event.pointerId}`, mask);
  };
  root.addEventListener("touchstart", event => event.preventDefault(), { passive: false });
  root.addEventListener("pointerdown", event => {
    if (event.button !== 0) return;
    const target = event.target.closest("[data-button], .dpad-hitbox");
    if (!target) return;
    event.preventDefault();
    const control = dpad.contains(target) ? dpad : target;
    pointers.set(event.pointerId, control);
    control.setPointerCapture(event.pointerId);
    update(event);
  });
  root.addEventListener("pointermove", event => {
    if (!pointers.has(event.pointerId)) return;
    event.preventDefault();
    update(event);
  });
  for (const name of ["pointerup", "pointercancel", "lostpointercapture"]) {
    root.addEventListener(name, event => {
      if (!pointers.has(event.pointerId)) return;
      event.preventDefault();
      pointers.delete(event.pointerId);
      input.set(`pointer-${event.pointerId}`, 0);
    });
  }
  // iOS callouts and Android secondary-click gestures must not interrupt holds.
  for (const name of ["contextmenu", "selectstart", "dragstart"]) root.addEventListener(name, event => event.preventDefault());
  return () => { pointers.clear(); };
}
