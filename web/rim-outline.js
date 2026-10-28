/** One aperture outline for the CSS opening, solid frame, and projected shadow. */
export const RIM_CORNER_FRACTION = 0.005;

export function traceRoundedRectangle(context, topLeft, bottomRight, rx, ry) {
  context.moveTo(topLeft.x + rx, topLeft.y);
  context.lineTo(bottomRight.x - rx, topLeft.y);
  context.ellipse(bottomRight.x - rx, topLeft.y + ry, rx, ry, 0, -Math.PI / 2, 0);
  context.lineTo(bottomRight.x, bottomRight.y - ry);
  context.ellipse(bottomRight.x - rx, bottomRight.y - ry, rx, ry, 0, 0, Math.PI / 2);
  context.lineTo(topLeft.x + rx, bottomRight.y);
  context.ellipse(topLeft.x + rx, bottomRight.y - ry, rx, ry, 0, Math.PI / 2, Math.PI);
  context.lineTo(topLeft.x, topLeft.y + ry);
  context.ellipse(topLeft.x + rx, topLeft.y + ry, rx, ry, 0, Math.PI, Math.PI * 1.5);
  context.closePath();
}
