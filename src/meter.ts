import { h, inputLevel, toDb } from "./api";

/** 指定裝置的即時音量條（每 200 ms 更新）；10 秒都低於 -80 dB 就提示沒有聲音。 */
export function meter(device: string): { el: HTMLElement; stop: () => void } {
  const bar = h("div", { class: "meter-fill" });
  const note = h("p", { class: "hint" });
  const el = h("div", { class: "meter-wrap" }, h("div", { class: "meter" }, bar), note);
  const started = Date.now();
  let loud = false;
  let alive = true;
  const tick = async () => {
    if (!alive) return;
    try {
      const db = toDb(await inputLevel(device));
      if (db > -80) loud = true;
      bar.style.width = `${Math.max(0, Math.min(100, ((db + 80) / 60) * 100))}%`;
      note.textContent = !loud && Date.now() - started > 10_000 ? "這支麥克風沒有聲音" : "";
    } catch (e) {
      note.textContent = String(e);
    }
    if (alive) setTimeout(tick, 200);
  };
  tick();
  return { el, stop: () => (alive = false) };
}
