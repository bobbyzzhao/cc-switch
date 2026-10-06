import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";

const mocks = vi.hoisted(() => ({
  version: "4.0.3-native-openai.1",
  install: vi.fn(),
  checkUpdates: vi.fn(),
  toastInfo: vi.fn(),
  toastError: vi.fn(),
  checkUpdate: vi.fn(),
  resetDismiss: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve(mocks.version),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: true,
    updateInfo: {
      currentVersion: mocks.version,
      availableVersion: "4.0.4",
      notes: "official release",
    },
    checkUpdate: mocks.checkUpdate,
    resetDismiss: mocks.resetDismiss,
    isChecking: false,
  }),
}));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    installUpdateAndRestart: (...args: unknown[]) => mocks.install(...args),
    checkUpdates: (...args: unknown[]) => mocks.checkUpdates(...args),
    openExternal: vi.fn(),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    info: (...args: unknown[]) => mocks.toastInfo(...args),
    error: (...args: unknown[]) => mocks.toastError(...args),
    success: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

describe("AboutSection fork update guard", () => {
  beforeEach(() => {
    mocks.version = "4.0.3-native-openai.1";
    mocks.install.mockReset();
    mocks.checkUpdates.mockReset();
    mocks.toastInfo.mockReset();
    mocks.toastError.mockReset();
  });

  it("blocks installing an official update from a native OpenAI fork", async () => {
    render(<AboutSection isPortable={false} />);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /settings.updateTo/ }),
      ).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole("button", { name: /settings.updateTo/ }));

    await waitFor(() => expect(mocks.toastInfo).toHaveBeenCalledTimes(1));
    expect(mocks.install).not.toHaveBeenCalled();
    expect(mocks.checkUpdates).not.toHaveBeenCalled();
  });

  it("keeps the official installer path for an upstream build", async () => {
    mocks.version = "4.0.3";
    mocks.install.mockResolvedValue(true);
    render(<AboutSection isPortable={false} />);
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /settings.updateTo/ }),
      ).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole("button", { name: /settings.updateTo/ }));
    await waitFor(() => expect(mocks.install).toHaveBeenCalledTimes(1));
    expect(mocks.toastInfo).not.toHaveBeenCalled();
  });
});
