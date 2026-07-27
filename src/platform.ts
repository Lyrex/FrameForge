import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ==============================================================================
// Platform capabilities
// ==============================================================================
//
// Not every backend implements every feature: OS-encrypted credential storage
// is Windows-only, and screen capture and OCR exist on Windows and Linux but
// nowhere else. The UI asks the backend what it can do instead of hiding
// features behind a user-agent guess, so a control is only offered when the
// command behind it can actually succeed.
//
// The query runs once per app load and every consumer shares that answer; a
// component tree this wide would otherwise thread the same three booleans
// through unrelated props.

export type PlatformCapabilities = {
  linux: boolean;
  ocr: boolean;
  persistentCredentials: boolean;
};

// Windows has been the only supported platform, so its capabilities are the
// optimistic default for the brief moment before the backend answers.
const WINDOWS: PlatformCapabilities = { linux: false, ocr: true, persistentCredentials: true };

let current = WINDOWS;
const ready = invoke<PlatformCapabilities>("get_platform_capabilities")
  .then(capabilities => (current = capabilities))
  .catch(() => current);

export function usePlatformCapabilities(): PlatformCapabilities {
  const [capabilities, setCapabilities] = useState(current);
  useEffect(() => {
    ready.then(setCapabilities);
  }, []);
  return capabilities;
}
