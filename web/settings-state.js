const owns = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
const STORAGE_KEY = "nes.presentation-defaults.v1";
const UNITS = {
  "depth-input": " game px", "thickness-input": " game px",
  "viewing-distance-input": " cm", "light-distance-input": " cm",
  "light-elevation-input": "°", "light-azimuth-input": "°",
};

export function controlValue(control) {
  return control.type === "checkbox" ? control.checked : control.value;
}

function factoryValue(control) {
  if (control.type === "checkbox") return control.defaultChecked;
  if (control.tagName === "SELECT") return [...control.options].find(option => option.defaultSelected)?.value ?? control.options[0].value;
  return control.defaultValue;
}

function validValue(control, value) {
  if (control.type === "checkbox") return typeof value === "boolean";
  if (typeof value !== "string") return false;
  if (control.tagName === "SELECT") return [...control.options].some(option => option.value === value);
  const number = Number(value), step = Number(control.step) || 1, min = Number(control.min);
  return value !== "" && Number.isFinite(number) && number >= min && number <= Number(control.max)
    && Math.abs((number - min) / step - Math.round((number - min) / step)) < 1e-8;
}

export class SettingsState {
  constructor(panel, { contextualDefault = () => undefined, summary, resetButton, factoryButton } = {}) {
    this.controls = [...panel.querySelectorAll("input, select")];
    this.contextualDefault = contextualDefault;
    this.summary = summary;
    this.resetButton = resetButton;
    this.factoryButton = factoryButton;
    this.preferred = {};
    this.saved = false;
    this.notes = new Map();
    for (const control of this.controls) {
      const label = control.closest("label");
      const note = document.createElement("small");
      note.className = "setting-default";
      note.id = `${control.id}-default`;
      note.hidden = true;
      label.append(note);
      if (control.type === "checkbox") label.classList.add("setting-toggle");
      control.setAttribute("aria-describedby", `${control.getAttribute("aria-describedby") || ""} ${note.id}`.trim());
      this.notes.set(control, { label, note });
    }
    try {
      const stored = JSON.parse(localStorage.getItem(STORAGE_KEY));
      if (stored?.version === 1 && stored.values && typeof stored.values === "object") {
        for (const control of this.controls) if (owns(stored.values, control.id) && validValue(control, stored.values[control.id])) this.preferred[control.id] = stored.values[control.id];
        this.saved = true;
      }
    } catch (_) { /* Storage is optional; controls remain usable. */ }
  }

  hasPreferred(id) { return owns(this.preferred, id); }
  factory(control) { return this.contextualDefault(control) ?? factoryValue(control); }
  defaultFor(control) { return owns(this.preferred, control.id) ? this.preferred[control.id] : this.factory(control); }
  snapshot() { return Object.fromEntries(this.controls.map(control => [control.id, controlValue(control)])); }
  defaults() { return Object.fromEntries(this.controls.map(control => [control.id, this.defaultFor(control)])); }

  apply(values) {
    for (const control of this.controls) {
      if (!owns(values, control.id) || !validValue(control, values[control.id])) continue;
      if (control.type === "checkbox") control.checked = values[control.id];
      else control.value = values[control.id];
    }
  }

  save() {
    this.preferred = {};
    for (const control of this.controls) {
      // Keep automatic eye-distance calibration adaptive across screen profiles.
      if (control.id !== "viewing-distance-input" || controlValue(control) !== this.factory(control)) this.preferred[control.id] = controlValue(control);
    }
    this.saved = true;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ version: 1, values: this.preferred }));
      return true;
    } catch (_) { return false; }
  }

  clear() {
    this.preferred = {};
    this.saved = false;
    try { localStorage.removeItem(STORAGE_KEY); return true; } catch (_) { return false; }
  }

  update() {
    let changed = 0;
    for (const control of this.controls) {
      const value = this.defaultFor(control);
      const modified = controlValue(control) !== value;
      const { label, note } = this.notes.get(control);
      if (modified) changed += 1;
      label.classList.toggle("is-modified", modified);
      control.dataset.modified = String(modified);
      note.hidden = !modified;
      const text = `${this.saved ? "Your default" : "Default"}: ${this.describe(control, value)}`;
      if (note.textContent !== text) note.textContent = text;
    }
    const text = changed ? `${changed} ${changed === 1 ? "setting changed" : "settings changed"}` : this.saved ? "At your preferred default" : "All settings at default";
    if (this.summary.textContent !== text) this.summary.textContent = text;
    this.resetButton.disabled = changed === 0;
    this.factoryButton.hidden = !this.saved;
    return changed;
  }

  describe(control, value) {
    if (control.type === "checkbox") return value ? "On" : "Off";
    if (control.tagName === "SELECT") return [...control.options].find(option => option.value === value)?.textContent.trim() ?? value;
    if (control.id === "light-azimuth-input" && Number(value) === 0) return "Centered";
    return `${value}${UNITS[control.id] ?? "%"}`;
  }
}
