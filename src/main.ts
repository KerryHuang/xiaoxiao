import { finishOnboarding, getSettings, setOnboardingOpen } from "./api";
import { onboarding } from "./onboarding";
import { settingsPage } from "./settings";

const root = document.getElementById("app")!;

async function show(forceOnboarding = false) {
  const s = await getSettings();
  if (!s.onboarded || forceOnboarding) {
    onboarding(root, s, async () => {
      await finishOnboarding();
      show();
    });
  } else {
    settingsPage(root, s, async () => {
      await setOnboardingOpen(true);
      show(true);
    });
  }
}

show();
