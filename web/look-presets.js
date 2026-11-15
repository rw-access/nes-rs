// Presets vary picture controls only. Sensor permission, camera calibration,
// audio, controller input and the loaded game are kept by the application.
const common = {
  "solid-sides-input": true, "sprite-relief-input": true, "sprite-relief-style": "contact",
  "thin-overlaps-input": true, "rim-input": false, "recessed-opening-input": false,
  "light-azimuth-input": "0", "diorama-input": true, "material-finish-input": "matte",
};
const look = (id, name, description, values) => Object.freeze({ id, name, description, values: Object.freeze({ ...common, ...values }) });
export const LOOK_PRESETS = Object.freeze([
  look("matte", "Paper diorama", "Matte layers, gentle shadows", {
    "depth-input": "100", "thickness-input": "5", "shading-strength-input": "35",
    "light-distance-input": "85", "light-elevation-input": "3", "softness-input": "9",
  }),
  look("crisp", "Crisp cutouts", "The same layers, stronger shadows", {
    "depth-input": "100", "thickness-input": "5", "shading-strength-input": "45",
    "light-distance-input": "85", "light-elevation-input": "3", "softness-input": "9",
  }),
  look("deep", "Deep cardboard", "Thicker edges, richer shadows", {
    "depth-input": "105", "thickness-input": "7", "shading-strength-input": "50",
    "light-distance-input": "85", "light-elevation-input": "3", "softness-input": "9",
  }),
  look("soft", "Soft paper", "Thinner layers, softer shadows", {
    "depth-input": "70", "thickness-input": "3", "shading-strength-input": "25",
    "light-distance-input": "110", "light-elevation-input": "4", "softness-input": "24",
  }),
  look("eggshell", "Eggshell", "Paper diorama with a soft sheen", {
    "depth-input": "100", "thickness-input": "5", "shading-strength-input": "35",
    "light-distance-input": "85", "light-elevation-input": "3", "softness-input": "9",
    "material-finish-input": "eggshell",
  }),
  look("satin", "Satin", "Paper diorama with a satin sheen", {
    "depth-input": "100", "thickness-input": "5", "shading-strength-input": "35",
    "light-distance-input": "85", "light-elevation-input": "3", "softness-input": "9",
    "material-finish-input": "satin",
  }),
]);

/** Original built-in paper-stage sample when no game is loaded. */
export function createPreviewScene() {
  const width = 256, height = 240, planeBytes = width * height * 4;
  const pixels = new Uint8ClampedArray(planeBytes * 4);
  const dot = (plane, x, y, rgb) => {
    if (x >= 0 && x < width && y >= 0 && y < height) pixels.set([...rgb, 255], plane * planeBytes + (y * width + x) * 4);
  };
  const rect = (plane, x, y, w, h, rgb) => {
    for (let py = y; py < y + h; py += 1) for (let px = x; px < x + w; px += 1) dot(plane, px, py, rgb);
  };
  const disc = (plane, x, y, radius, rgb) => {
    for (let dy = -radius; dy <= radius; dy += 1) for (let dx = -radius; dx <= radius; dx += 1) if (dx * dx + dy * dy <= radius * radius) dot(plane, x + dx, y + dy, rgb);
  };
  rect(0, 0, 0, width, height, [137, 190, 216]);
  for (const [x, y] of [[50,62],[185,42]]) {
    disc(2, x, y, 11, [245,242,224]); disc(2, x+14, y-6, 15, [245,242,224]); disc(2, x+29, y, 10, [245,242,224]);
  }
  for (let y = 150; y < 214; y += 1) rect(2, 175 - (y - 150), y, 1 + (y - 150) * 2, 1, [85,147,101]);
  rect(2, 29, 171, 10, 43, [154,102,64]);
  disc(2, 34, 162, 21, [73,130,88]); disc(2, 24, 146, 16, [98,160,107]); disc(2, 47, 145, 17, [98,160,107]);
  rect(2, 125, 169, 27, 45, [220,133,87]);
  rect(2, 121, 160, 35, 12, [236,158,105]);
  rect(2, 128, 175, 5, 36, [241,177,120]);
  for (let y = 214; y < 240; y += 1) for (let x = 0; x < width; x += 1) {
    dot(2, x, y, (x % 16 === 0 || (y - 214) % 13 === 0) ? [235,200,142] : ((x / 16 | 0) + (y / 13 | 0)) % 2 ? [169,109,67] : [185,122,75]);
  }
  // A little wooden robot, with distinct painted faces for finish comparison.
  rect(3, 83, 188, 8, 26, [73,88,110]); rect(3, 97, 188, 8, 26, [73,88,110]);
  rect(3, 79, 166, 30, 27, [68,147,166]); rect(3, 72, 170, 7, 21, [227,176,104]);
  rect(3, 109, 170, 7, 21, [227,176,104]); rect(3, 83, 142, 22, 22, [239,197,119]);
  rect(3, 87, 150, 3, 3, [61,67,78]); rect(3, 98, 150, 3, 3, [61,67,78]); rect(3, 92, 157, 6, 2, [165,104,68]);
  rect(3, 88, 166, 14, 5, [207,103,93]);
  const glyphs = ["110101110100100", "100100100100111", "010101111101101", "101101010010010"];
  glyphs.forEach((glyph, letter) => [...glyph].forEach((bit, n) => { if (bit === "1") rect(2, 16 + letter * 16 + n % 3 * 3, 17 + (n / 3 | 0) * 3, 3, 3, [248,242,218]); }));
  return pixels;
}
