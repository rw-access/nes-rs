// A small dielectric highlight layered over printed artwork, not an environment
// reflection renderer. GGX distribution, separable Smith visibility and Schlick
// Fresnel follow glTF 2.0 Appendix B (dielectric F0 = 0.04):
// https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#appendix-b-brdf-implementation
// Exposure, maximum white coverage and the narrow grazing fade are artistic
// display controls; the result is not a calibrated radiometric measurement.
export const MATTE = Object.freeze({ name: "matte", label: "Matte", roughness: 1, exposure: 0, maxGlint: 0 });
export const EGGSHELL = Object.freeze({ name: "eggshell", label: "Eggshell", roughness: 0.68, exposure: 0.85, maxGlint: 0.06 });
export const SATIN = Object.freeze({ name: "satin", label: "Satin", roughness: 0.45, exposure: 0.85, maxGlint: 0.14 });
export const MATERIAL_FINISHES = Object.freeze({ matte: MATTE, eggshell: EGGSHELL, satin: SATIN });

const DEPTH_PIXELS = 19.2;
const FRONT_NORMAL = Object.freeze({ x: 0, y: 0, z: -1 });
const ZERO_GLINT = () => 0;
let materialFinish = "matte";
let materialRevision = 0;

export function getMaterialFinish() { return materialFinish; }
export function getMaterialRevision() { return materialRevision; }

export function setMaterialFinish(name) {
  if (typeof name !== "string" || !Object.prototype.hasOwnProperty.call(MATERIAL_FINISHES, name)) {
    throw new RangeError("Material finish must be matte, eggshell, or satin");
  }
  if (name === materialFinish) return false;
  materialFinish = name;
  materialRevision += 1;
  return true;
}

/**
 * Snapshot the selected finish and light/eye positions into an allocation-free
 * sampler. Recreate it when material, lighting, view or strength changes.
 * x/y use the supplied canvas dimensions; z uses scene depth units. Positions
 * become native-pixel coordinates, with 19.2 pixels per depth unit. Supplied
 * normals use those same input axes and undergo the inverse-transpose scale.
 * The ordinary front normal is optimized. Matte and zero strength have no
 * per-sample work and do not require light/eye positions.
 */
export function createMaterialSampler({ light, eye, width = 256, height = 240, strength = 1 } = {}) {
  if (!Number.isFinite(width) || width <= 0 || !Number.isFinite(height) || height <= 0) {
    throw new RangeError("Material dimensions must be positive finite numbers");
  }
  if (!Number.isFinite(strength) || strength < 0 || strength > 1) {
    throw new RangeError("Material strength must be from 0 to 1");
  }
  const profile = MATERIAL_FINISHES[materialFinish];
  if (profile === MATTE || strength === 0) return ZERO_GLINT;
  validatePosition(light, "Light");
  validatePosition(eye, "Eye");

  const scaleX = 256 / width, scaleY = 240 / height;
  const lightX = light.x * scaleX, lightY = light.y * scaleY, lightZ = light.z * DEPTH_PIXELS;
  const eyeX = eye.x * scaleX, eyeY = eye.y * scaleY, eyeZ = eye.z * DEPTH_PIXELS;
  // glTF maps perceptual roughness to alpha = roughness squared.
  const alphaSquared = profile.roughness ** 4;
  const alphaComplement = 1 - alphaSquared;
  const distributionScale = alphaSquared / Math.PI;
  const exposure = profile.exposure;
  const cap = Math.min(0.18, profile.maxGlint) * strength;

  return (x, y, z = 0, normal = FRONT_NORMAL) => {
    const px = x * scaleX, py = y * scaleY, pz = z * DEPTH_PIXELS;
    let lx = lightX - px, ly = lightY - py, lz = lightZ - pz;
    let vx = eyeX - px, vy = eyeY - py, vz = eyeZ - pz;
    const lightLength = Math.sqrt(lx * lx + ly * ly + lz * lz);
    const eyeLength = Math.sqrt(vx * vx + vy * vy + vz * vz);
    if (!(lightLength > 0) || !Number.isFinite(lightLength)
      || !(eyeLength > 0) || !Number.isFinite(eyeLength)) return 0;
    const inverseLightLength = 1 / lightLength, inverseEyeLength = 1 / eyeLength;
    lx *= inverseLightLength; ly *= inverseLightLength; lz *= inverseLightLength;
    vx *= inverseEyeLength; vy *= inverseEyeLength; vz *= inverseEyeLength;

    let nx = 0, ny = 0, nz = -1;
    if (normal !== FRONT_NORMAL && !(normal?.x === 0 && normal.y === 0 && normal.z < 0 && Number.isFinite(normal.z))) {
      nx = normal?.x / scaleX; ny = normal?.y / scaleY; nz = normal?.z / DEPTH_PIXELS;
      const normalLength = Math.sqrt(nx * nx + ny * ny + nz * nz);
      if (!(normalLength > 0) || !Number.isFinite(normalLength)) return 0;
      nx /= normalLength; ny /= normalLength; nz /= normalLength;
    }
    const noL = Math.min(1, nx * lx + ny * ly + nz * lz);
    const noV = Math.min(1, nx * vx + ny * vy + nz * vz);
    if (!(noL > 0) || !(noV > 0)) return 0;

    let hx = lx + vx, hy = ly + vy, hz = lz + vz;
    const halfLength = Math.sqrt(hx * hx + hy * hy + hz * hz);
    if (!(halfLength > 0)) return 0;
    hx /= halfLength; hy /= halfLength; hz /= halfLength;
    const noH = Math.max(0, Math.min(1, nx * hx + ny * hy + nz * hz));
    const voH = Math.max(0, Math.min(1, vx * hx + vy * hy + vz * hz));
    const denominator = 1 - noH * noH * alphaComplement;
    const distribution = distributionScale / (denominator * denominator);
    // G / (4 NoL NoV), with the common factors cancelled before evaluation.
    const visibility = 1 / ((noL + Math.sqrt(alphaSquared + alphaComplement * noL * noL))
      * (noV + Math.sqrt(alphaSquared + alphaComplement * noV * noV)));
    const fresnelBase = 1 - voH, fresnelSquared = fresnelBase * fresnelBase;
    const fresnel = 0.04 + 0.96 * fresnelSquared * fresnelSquared * fresnelBase;
    // Suppress razor-bright silhouettes smoothly before either ray crosses
    // behind the surface; neither a backlight nor a back-facing eye can glint.
    const lightFade = Math.min(1, noL / 0.08), eyeFade = Math.min(1, noV / 0.08);
    const fade = lightFade * lightFade * (3 - 2 * lightFade) * eyeFade * eyeFade * (3 - 2 * eyeFade);
    const reflected = distribution * visibility * fresnel * noL * exposure * fade * strength;
    return Math.min(cap, Math.max(0, reflected));
  };
}

function validatePosition(position, label) {
  if (!position || !Number.isFinite(position.x) || !Number.isFinite(position.y) || !Number.isFinite(position.z)) {
    throw new TypeError(`${label} position must have finite x, y, and z`);
  }
}
