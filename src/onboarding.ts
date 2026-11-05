import {
  IS_WINDOWS,
  type InputDevice,
  type Settings,
  checkClaude,
  checkVoice,
  ensureModels,
  h,
  inputLevel,
  listInputs,
  micPermission,
  saveSettings,
  stopInputLevel,
  testWake,
} from "./api";
import { listen } from "@tauri-apps/api/event";
import { meter } from "./meter";

const BT_NOTE = "用藍牙耳機收音會讓耳機變成通話音質";

/** 第一次啟動引導五步。第 1（麥克風權限）、4（Claude）步沒過不能開始待命。 */
export function onboarding(root: HTMLElement, settings: Settings, done: () => void) {
  const passed = new Set<number>();
  let step = 1;
  let cleanup: (() => void) | undefined;

  const render = () => {
    cleanup?.();
    cleanup = undefined;
    root.replaceChildren();
    const dots = h("ol", { class: "steps" });
    for (let i = 1; i <= 5; i++) {
      dots.append(h("li", { class: i === step ? "now" : passed.has(i) ? "ok" : "" }, String(i)));
    }
    const body = h("section", { class: "card" });
    root.append(h("h1", {}, "設定小小"), dots, body);
    [stepMic, stepDevice, stepVoice, stepClaude, stepWake][step - 1](body);
  };

  const nav = (body: HTMLElement) => {
    const row = h("div", { class: "row" });
    if (step > 1) row.append(h("button", { class: "ghost", onclick: () => ((step -= 1), render()) }, "上一步"));
    if (step < 5) {
      row.append(h("button", { class: "ghost", onclick: () => ((step += 1), render()) }, "稍後再說"));
    } else {
      const ready = passed.has(1) && passed.has(4);
      const start = h("button", { disabled: !ready, onclick: finish }, "開始待命");
      row.append(start);
      if (!ready) body.append(h("p", { class: "hint" }, "要先完成第 1 步（麥克風）和第 4 步（Claude）才能開始待命。"));
    }
    body.append(row);
  };

  const pass = (n: number) => {
    passed.add(n);
    step = Math.min(5, n + 1);
    render();
  };

  const finish = async () => {
    await saveSettings({ ...settings, onboarded: true });
    done();
  };

  // 1. 麥克風權限
  const stepMic = (body: HTMLElement) => {
    const msg = h("p", { class: "hint" });
    body.append(
      h("h2", {}, "1. 允許使用麥克風"),
      h("p", {}, `小小要一直聽著才聽得到「嘿，小小」，所以${IS_WINDOWS ? "工作列的麥克風圖示" : "選單列的橘色麥克風燈號"}會一直亮著。聲音只在這台電腦上處理，不會存檔或上傳；開會、通話時會自動暫停。`),
      h(
        "button",
        {
          onclick: async () => {
            msg.textContent = IS_WINDOWS ? "檢查中…" : "請在系統視窗按「允許」…";
            try {
              const devs = await listInputs();
              const d = devs.find((x) => !x.bluetooth);
              if (!d) {
                msg.textContent = IS_WINDOWS
                  ? "這台電腦目前看不到可用的麥克風（藍牙耳機不算）。請確認麥克風已接上；用遠端桌面連線時收不到本機麥克風，請到那台電腦前操作，或在遠端桌面的設定開啟麥克風轉接。"
                  : "找不到可用的麥克風（藍牙耳機不算），請確認麥克風已接上。";
                return;
              }
              // 開一次麥克風讓系統跳出詢問；被拒時 CoreAudio 不報錯，所以要另外問授權狀態
              if ((await micPermission()) === "undetermined") await inputLevel(d.name);
              let p = await micPermission();
              for (let i = 0; p === "undetermined" && i < 120; i++) {
                await new Promise((r) => setTimeout(r, 500));
                p = await micPermission();
              }
              await stopInputLevel();
              if (p !== "authorized") throw new Error(p === "undetermined" ? "還沒有回應系統的詢問" : "麥克風權限被關閉");
              pass(1);
            } catch (e) {
              msg.textContent = IS_WINDOWS
                ? `沒有成功：${e}。可到「設定 → 隱私權與安全性 → 麥克風」，打開最下方的「允許傳統型應用程式存取您的麥克風」（Windows 10 叫「允許桌面應用程式存取您的麥克風」）。`
                : `沒有成功：${e}。可到「系統設定 → 隱私權與安全性 → 麥克風」打開小小。`;
            }
          },
        },
        "允許使用麥克風",
      ),
      msg,
    );
    nav(body);
  };

  // 2. 選收音麥克風
  const stepDevice = (body: HTMLElement) => {
    body.append(h("h2", {}, "2. 選收音的麥克風"));
    const list = h("div", { class: "devices" });
    body.append(list);
    let stopMeter: (() => void) | undefined;
    cleanup = () => {
      stopMeter?.();
      stopInputLevel();
    };
    const draw = (devs: InputDevice[]) => {
      list.replaceChildren();
      stopMeter?.();
      for (const d of devs) {
        const selected = settings.input_device === d.name;
        const item = h("label", { class: `device${d.bluetooth ? " off" : ""}` });
        const radio = h("input", { type: "radio", name: "dev", checked: selected, disabled: d.bluetooth });
        radio.onchange = () => {
          settings = { ...settings, input_device: d.name };
          draw(devs);
        };
        item.append(radio, h("span", {}, d.name));
        if (d.bluetooth) item.append(h("small", {}, BT_NOTE));
        list.append(item);
        if (selected && !d.bluetooth) {
          const m = meter(d.name);
          stopMeter = m.stop;
          list.append(m.el);
        }
      }
    };
    listInputs().then(draw);
    body.append(h("p", { class: "hint" }, "不選的話，小小會用系統預設麥克風；預設是藍牙時改用內建或 USB 麥克風。"));
    const row = h("div", { class: "row" });
    row.append(h("button", { onclick: async () => (await saveSettings(settings), pass(2)) }, "用這支"));
    body.append(row);
    nav(body);
  };

  // 3. 語音
  const stepVoice = (body: HTMLElement) => {
    const msg = h("p");
    body.append(h("h2", {}, "3. 小小的聲音"), msg);
    checkVoice().then((ok) => {
      if (ok) {
        msg.textContent = IS_WINDOWS ? "已安裝台灣國語語音。" : "已安裝「美佳」語音。";
        passed.add(3);
      } else {
        msg.textContent = IS_WINDOWS
          ? "還沒有台灣國語語音：到「設定 → 時間與語言 → 語言與地區」新增「中文（繁體，台灣）」並勾選語音，再按重新檢查。"
          : "還沒有「美佳」語音：到「系統設定 → 輔助使用 → 閱讀與朗讀 → 系統聲音旁的 ⓘ → 聲音」，搜尋並下載「美佳」，再按重新檢查。";
      }
    });
    body.append(h("button", { class: "ghost", onclick: render }, "重新檢查"));
    nav(body);
  };

  // 4. Claude
  const stepClaude = (body: HTMLElement) => {
    const msg = h("p");
    body.append(h("h2", {}, "4. 連上 Claude"), msg);
    msg.textContent = "檢查中…";
    checkClaude()
      .then((ver) => {
        msg.textContent = `找到 Claude Code ${ver}，已登入。`;
        passed.add(4);
        nav(body);
      })
      .catch((e) => {
        msg.textContent = `${e} 請在 Claude Code 或 Claude Desktop 自己登入，小小不會碰你的帳號。`;
        nav(body);
      });
    body.append(h("button", { class: "ghost", onclick: render }, "重新檢查"));
  };

  // 5. 試喊
  const stepWake = (body: HTMLElement) => {
    const msg = h("p");
    const btn = h(
      "button",
      {
        onclick: async () => {
          btn.disabled = true;
          msg.textContent = "請在 10 秒內說「嘿，小小」…";
          try {
            const ok = await testWake();
            msg.textContent = ok
              ? "✓ 聽到了！"
              : `沒有聽到。可以再試一次，或先用快捷鍵（${settings.hotkey}）叫小小。`;
            if (ok) passed.add(5);
          } catch (e) {
            msg.textContent = String(e);
          }
          btn.disabled = false;
        },
      },
      "開始試喊",
    );
    body.append(h("h2", {}, "5. 試喊一次"), h("p", {}, "按下按鈕後，用平常的音量說「嘿，小小」。"), btn, msg);
    nav(body);
  };

  // 0. 先把模型準備好（約 1.1 GB，只需下載一次）
  const download = async () => {
    const bar = h("div", { class: "meter-fill" });
    const msg = h("p", { class: "hint" }, "第一次使用要下載語音模型（約 1.1 GB），只需要一次。");
    root.replaceChildren(h("h1", {}, "設定小小"),
      h("section", { class: "card" }, h("h2", {}, "準備語音模型"), h("div", { class: "meter" }, bar), msg));
    const un = await listen<{ done: number; total: number }>("models-progress", (e) => {
      const { done, total } = e.payload;
      if (total > 0) bar.style.width = `${(done / total) * 100}%`;
      msg.textContent = `下載中… ${(done / 1e6).toFixed(0)} MB${total > 0 ? ` / ${(total / 1e6).toFixed(0)} MB` : ""}`;
    });
    try {
      await ensureModels();
      un();
      render();
    } catch (e) {
      un();
      msg.textContent = `下載失敗：${e}`;
      root.querySelector("section")!.append(h("button", { onclick: download }, "重試"));
    }
  };
  download();
}
