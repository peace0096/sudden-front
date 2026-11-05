// Personal settings, saved in this browser. Storage can be missing, blocked or full, so every
// access is guarded and the game works on the defaults without it.

const SETTINGS_KEY = "sudden-front-settings";

// The mouse sensitivity is a multiplier on the game's base look rate (MOUSE_SENSITIVITY in
// game.ts), so 1 is the rate the game shipped with.
export const SENSITIVITY_MIN = 0.1;
export const SENSITIVITY_MAX = 5;
export const SENSITIVITY_STEP = 0.05;
export const SENSITIVITY_DEFAULT = 1;
// Every step is a multiple of 0.01, so two decimals remove the float noise of k * step.
const SENSITIVITY_DECIMALS = 2;

let sensitivity: number | null = null;
// Each control on screen, with the function that redraws it for a new value.
const controls = new Map<HTMLElement, (value: number) => void>();

function snap(value: number): number {
  const clamped = Math.min(SENSITIVITY_MAX, Math.max(SENSITIVITY_MIN, value));
  const stepped = Math.round(clamped / SENSITIVITY_STEP) * SENSITIVITY_STEP;
  return Number(stepped.toFixed(SENSITIVITY_DECIMALS));
}

function loadMouseSensitivity(): number {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    const value = (stored as { mouseSensitivity?: unknown } | null)?.mouseSensitivity;
    // Anything but a number inside the slider's range is not something this page wrote.
    if (typeof value === "number" && value >= SENSITIVITY_MIN && value <= SENSITIVITY_MAX) return snap(value);
  } catch {
    // Unreadable or garbled storage just means the default.
  }
  return SENSITIVITY_DEFAULT;
}

function saveMouseSensitivity(value: number): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ mouseSensitivity: value }));
  } catch {
    // Remembering the setting is only a convenience.
  }
}

// A control leaves the document with its screen: the lobby redraws, the HUD goes with its match.
function dropDetachedControls(): void {
  for (const root of controls.keys()) {
    if (!root.isConnected) controls.delete(root);
  }
}

/** The player's look-rate multiplier; 1 is the game's base rate. */
export function mouseSensitivity(): number {
  sensitivity ??= loadMouseSensitivity();
  return sensitivity;
}

/**
 * Clamps and snaps the value to the slider's grid, saves it and redraws every control on screen.
 * Returns the stored value. A value that is not a finite number keeps the current one.
 */
export function setMouseSensitivity(value: number): number {
  if (Number.isFinite(value)) {
    sensitivity = snap(value);
    saveMouseSensitivity(sensitivity);
  }
  const stored = mouseSensitivity();
  dropDetachedControls();
  for (const redraw of controls.values()) redraw(stored);
  return stored;
}

/**
 * A slider, a number box and a reset button for the mouse sensitivity. All of them on screen
 * follow the stored value. Attach the element before building another one: a control that is not
 * in the document yet counts as gone.
 */
export function sensitivityControl(): HTMLElement {
  dropDetachedControls();
  const root = document.createElement("div");
  root.className = "sensitivity";

  const label = document.createElement("span");
  label.className = "sensitivity-label";
  label.textContent = "마우스 감도";

  const range = document.createElement("input");
  range.type = "range";
  range.setAttribute("aria-label", "마우스 감도");

  const box = document.createElement("input");
  box.type = "number";
  box.className = "field";
  box.setAttribute("aria-label", "마우스 감도 값");

  // The bounds go in before any value, or the browser rounds the value to its default scale.
  for (const input of [range, box]) {
    input.min = String(SENSITIVITY_MIN);
    input.max = String(SENSITIVITY_MAX);
    input.step = String(SENSITIVITY_STEP);
  }

  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "btn";
  reset.textContent = "기본값";

  range.addEventListener("input", () => setMouseSensitivity(range.valueAsNumber));
  // Typed text applies when it is committed, so a half-typed "0." is never clamped mid-keystroke.
  // An empty or unreadable box is not a number: the stored value is drawn back over it.
  box.addEventListener("change", () => setMouseSensitivity(box.valueAsNumber));
  reset.addEventListener("click", () => setMouseSensitivity(SENSITIVITY_DEFAULT));

  const redraw = (value: number) => {
    range.value = String(value);
    box.value = value.toFixed(SENSITIVITY_DECIMALS);
  };
  root.append(label, range, box, reset);
  redraw(mouseSensitivity());
  controls.set(root, redraw);
  return root;
}
