import { open } from "@tauri-apps/plugin-dialog";
import { type Settings, h, listInputs, saveSettings } from "./api";

/** 設定頁。儲存後引擎會重新啟動。 */
export function settingsPage(root: HTMLElement, initial: Settings, rerunOnboarding: () => void) {
  let s: Settings = structuredClone(initial);
  const status = h("p", { class: "hint" });

  const field = (label: string, ...control: (Node | string)[]) =>
    h("div", { class: "field" }, h("label", {}, label), ...control);

  // 收音麥克風
  const dev = h("select");
  listInputs().then((devs) => {
    dev.append(h("option", { value: "", selected: s.input_device === null }, "系統預設（藍牙時自動改用內建或 USB）"));
    for (const d of devs) {
      dev.append(
        h("option", { value: d.name, disabled: d.bluetooth, selected: s.input_device === d.name },
          d.bluetooth ? `${d.name}（藍牙：會變成通話音質）` : d.name),
      );
    }
  });
  dev.onchange = () => (s.input_device = dev.value || null);

  // 快捷鍵
  const hotkey = h("input", { value: s.hotkey });
  hotkey.oninput = () => (s.hotkey = hotkey.value.trim());

  // 可讀資料夾
  const dirs = h("ul", { class: "dirs" });
  const drawDirs = () => {
    dirs.replaceChildren(
      ...s.read_dirs.map((d, i) =>
        h("li", {}, h("span", {}, d),
          h("button", { class: "ghost small", onclick: () => (s.read_dirs.splice(i, 1), drawDirs()) }, "移除")),
      ),
    );
  };
  drawDirs();
  const addDir = h("button", {
    class: "ghost",
    onclick: async () => {
      const picked = await open({ directory: true, multiple: false });
      if (typeof picked === "string" && !s.read_dirs.includes(picked)) {
        s.read_dirs.push(picked);
        drawDirs();
      }
    },
  }, "加入資料夾…");

  // 筆記檔
  const notes = h("input", { value: s.notes_path });
  notes.oninput = () => (s.notes_path = notes.value.trim());

  // 診斷模式
  const diagWarn = h("p", { class: "danger" }, "會記錄你說的每一句話，測完請關閉並刪除紀錄。");
  const diag = h("input", { type: "checkbox", checked: s.diagnostic });
  const syncDiag = () => (diagWarn.hidden = !diag.checked);
  diag.onchange = () => ((s.diagnostic = diag.checked), syncDiag());
  syncDiag();

  // 回答引擎
  const engine = h("div", { class: "radios" },
    h("label", {}, h("input", { type: "radio", name: "engine", checked: s.engine === "ClaudeCli", onchange: () => (s.engine = "ClaudeCli") }), " Claude Code（用你自己登入的帳號）"),
    h("label", { class: "off" }, h("input", { type: "radio", name: "engine", disabled: true }), " API key（之後支援）"),
  );

  const save = h("button", {
    onclick: async () => {
      if (!s.hotkey) return void (status.textContent = "快捷鍵不能空白");
      try {
        await saveSettings(s);
        status.textContent = "已儲存，小小重新啟動中。";
      } catch (e) {
        status.textContent = `儲存失敗：${e}`;
      }
    },
  }, "儲存");

  root.replaceChildren(
    h("h1", {}, "小小設定"),
    h("section", { class: "card" },
      field("收音麥克風", dev),
      field("快捷鍵", hotkey, h("small", {}, "例如 Control+Option+Space")),
      field("可讀資料夾",
        h("p", { class: "hint" }, "這些資料夾的內容可能會送到 Claude；金鑰、.env、.ssh 一律不讀。"),
        dirs, addDir),
      field("筆記檔位置", notes),
      field("診斷模式", h("label", {}, diag, " 開啟"), diagWarn),
      field("回答引擎", engine),
      h("div", { class: "row" }, save, h("button", { class: "ghost", onclick: rerunOnboarding }, "重新走一次引導")),
      status,
    ),
  );
}
