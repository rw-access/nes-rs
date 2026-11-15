import { mkdir } from "node:fs/promises";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
export const base = process.env.NES_PREVIEW_URL || "http://127.0.0.1:8765/";
export async function launch() {
  await mkdir("/tmp/nes-shared-check", { recursive: true });
  return chromium.launch({ executablePath: process.env.CHROMIUM_PATH || "/usr/bin/chromium", headless: true, args: ["--no-sandbox", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
}
export async function settings(page) { await page.locator("#effects-settings-button").click(); }
export async function range(page, id, value) { await page.locator(`#${id}`).evaluate((input, value) => { input.value = String(value); input.dispatchEvent(new Event("input", { bubbles: true })); }, value); }
