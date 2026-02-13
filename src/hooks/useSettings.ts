import { useState, useCallback, useEffect } from "react";
import type { AppSettings } from "../lib/types";
import { DEFAULT_SETTINGS } from "../lib/types";
import * as api from "../lib/tauri";

export function useSettings() {
  const [settings, setSettingsState] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        setSettingsState(s);
        setLoaded(true);
      })
      .catch(() => setLoaded(true));
  }, []);

  const updateSettings = useCallback((partial: Partial<AppSettings>) => {
    setSettingsState((prev) => {
      const next = { ...prev, ...partial };
      api.setSettings(next).catch(() => {});
      return next;
    });
  }, []);

  return { settings, updateSettings, loaded };
}
