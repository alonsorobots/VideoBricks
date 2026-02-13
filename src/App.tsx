import { useState, useCallback, useEffect, useRef } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import StartScreen from "./screens/StartScreen";
import EditScreen from "./screens/EditScreen";
import ConversionScreen from "./screens/ConversionScreen";
import CompletedScreen from "./screens/CompletedScreen";
import type {
  AppScreen,
  VideoMetadata,
  ConversionSettings,
  ConversionProgressEvent,
  AppSettings,
  EditState,
} from "./lib/types";
import { DEFAULT_SETTINGS } from "./lib/types";
import * as api from "./lib/tauri";

const pageVariants = {
  initial: { opacity: 0, y: 12 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -12 },
};

const pageTransition = {
  duration: 0.25,
  ease: [0.4, 0, 0.2, 1],
};

export default function App() {
  const [screen, setScreen] = useState<AppScreen>({ type: "start" });
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Persist edit-level state across screen transitions (trim, crop, dimensions)
  const editStateRef = useRef<EditState | null>(null);

  // Load persisted settings on mount
  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => { });
  }, []);

  // Resize window based on current screen, proportional to monitor resolution.
  // Your tweaked values on a 1920x1080 monitor were:
  //   start: 600x600  => 31.25% width, 55.56% height
  //   edit:  1920x1080 => 100% width, 100% height
  useEffect(() => {
    const resize = async () => {
      const appWindow = getCurrentWindow();
      const sw = window.screen.width;
      const sh = window.screen.height;

      if (screen.type === "start") {
        const w = Math.round(sw * 0.2025);
        const h = Math.round(sh * 0.3800);
        await appWindow.setMinSize(new LogicalSize(420, 320));
        await appWindow.setSize(new LogicalSize(w, h));
        await appWindow.center();
      } else if (screen.type === "edit") {
        const w = Math.round(sw * 0.8);
        const h = Math.round(sh * 0.8);
        await appWindow.setMinSize(new LogicalSize(640, 480));
        await appWindow.setSize(new LogicalSize(w, h));
        await appWindow.center();
      }
    };
    resize().catch(console.error);
  }, [screen.type]);

  // Listen for conversion progress events
  useEffect(() => {
    const unlisten = listen<ConversionProgressEvent>(
      "conversion-progress",
      (event) => {
        setProgress(event.payload.progress);
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Persist settings when they change
  const updateSettings = useCallback(
    (newSettings: AppSettings) => {
      setSettings(newSettings);
      api.setSettings(newSettings).catch(() => { });
    },
    []
  );

  const handleVideoSelected = useCallback(
    (videoPath: string, metadata: VideoMetadata) => {
      setError(null);
      editStateRef.current = null; // New video, fresh edit state
      setScreen({ type: "edit", videoPath, metadata });
    },
    []
  );

  const handleStartConversion = useCallback(
    async (conversionSettings: ConversionSettings) => {
      // Capture the current video context so we can return to it
      const currentScreen = screen;
      const videoPath = currentScreen.type === "edit" ? currentScreen.videoPath : conversionSettings.sourcePath;
      const metadata = currentScreen.type === "edit" ? currentScreen.metadata : null;

      setScreen({ type: "converting", settings: conversionSettings, format: "gif" });
      setProgress(0);
      setError(null);

      try {
        const gifs = await api.convertToGif(conversionSettings);
        setScreen({
          type: "completed",
          gifs,
          mp4s: [],
          format: "gif",
          videoPath,
          metadata: metadata!,
          outputName: conversionSettings.outputName,
        });
      } catch (err: unknown) {
        const message =
          err instanceof Error ? err.message : String(err);
        if (message.includes("cancelled")) {
          if (metadata) {
            setScreen({ type: "edit", videoPath, metadata });
          } else {
            setScreen({ type: "start" });
          }
        } else {
          setError(message);
          if (metadata) {
            setScreen({ type: "edit", videoPath, metadata });
          } else {
            setScreen({ type: "start" });
          }
        }
      }
    },
    [screen]
  );

  const handleStartMp4Export = useCallback(
    async (conversionSettings: ConversionSettings) => {
      const currentScreen = screen;
      const videoPath = currentScreen.type === "edit" ? currentScreen.videoPath : conversionSettings.sourcePath;
      const metadata = currentScreen.type === "edit" ? currentScreen.metadata : null;

      // Use user-defined output name from conversion settings
      const baseName = conversionSettings.outputName || "video";

      setScreen({ type: "converting", settings: conversionSettings, format: "mp4" });
      setProgress(0);
      setError(null);

      try {
        // Convert to temp directory first (like GIF), user saves afterwards
        const tempDir = await api.getTempDir();
        const outputPath = `${tempDir}\\${baseName}.mp4`;

        const mp4s = await api.convertToMp4(outputPath, conversionSettings);
        setScreen({
          type: "completed",
          gifs: [],
          mp4s,
          format: "mp4",
          videoPath,
          metadata: metadata!,
          outputName: conversionSettings.outputName,
        });
      } catch (err: unknown) {
        const message =
          err instanceof Error ? err.message : String(err);
        if (message.includes("cancelled")) {
          if (metadata) {
            setScreen({ type: "edit", videoPath, metadata });
          } else {
            setScreen({ type: "start" });
          }
        } else {
          setError(message);
          if (metadata) {
            setScreen({ type: "edit", videoPath, metadata });
          } else {
            setScreen({ type: "start" });
          }
        }
      }
    },
    [screen]
  );

  const handleCancelConversion = useCallback(async () => {
    await api.cancelConversion();
    // Try to go back to edit screen if possible
    // The conversion screen has settings with sourcePath, but we need metadata
    // editStateRef still has the edit state, we'll rely on the screen transition
    setScreen({ type: "start" });
  }, []);

  const handleNewConversion = useCallback(() => {
    editStateRef.current = null;
    setScreen({ type: "start" });
    setProgress(0);
    setError(null);
  }, []);

  const handleBackToEdit = useCallback(() => {
    if (screen.type === "completed") {
      setScreen({
        type: "edit",
        videoPath: screen.videoPath,
        metadata: screen.metadata,
      });
    }
  }, [screen]);

  const handleEditStateChange = useCallback((state: EditState) => {
    editStateRef.current = state;
  }, []);

  return (
    <div className="h-full flex flex-col bg-bg">
      {/* Error banner */}
      <AnimatePresence>
        {error && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="bg-danger/10 border-b border-danger/20 px-4 py-2 text-sm text-danger flex items-center justify-between"
          >
            <span>{error}</span>
            <button
              onClick={() => setError(null)}
              className="ml-2 text-danger hover:text-danger/80 font-medium"
            >
              Dismiss
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Main content */}
      <div className="flex-1 overflow-hidden relative">
        <AnimatePresence mode="wait">
          {screen.type === "start" && (
            <motion.div
              key="start"
              variants={pageVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={pageTransition}
              className="absolute inset-0"
            >
              <StartScreen onVideoSelected={handleVideoSelected} />
            </motion.div>
          )}

          {screen.type === "edit" && (
            <motion.div
              key="edit"
              variants={pageVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={pageTransition}
              className="absolute inset-0"
            >
              <EditScreen
                videoPath={screen.videoPath}
                metadata={screen.metadata}
                settings={settings}
                onSettingsChange={updateSettings}
                onConvert={handleStartConversion}
                onConvertMp4={handleStartMp4Export}
                onCancel={handleNewConversion}
                initialEditState={editStateRef.current}
                onEditStateChange={handleEditStateChange}
              />
            </motion.div>
          )}

          {screen.type === "converting" && (
            <motion.div
              key="converting"
              variants={pageVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={pageTransition}
              className="absolute inset-0"
            >
              <ConversionScreen
                progress={progress}
                format={screen.format}
                onCancel={handleCancelConversion}
              />
            </motion.div>
          )}

          {screen.type === "completed" && (
            <motion.div
              key="completed"
              variants={pageVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={pageTransition}
              className="absolute inset-0"
            >
              <CompletedScreen
                gifs={screen.gifs}
                mp4s={screen.mp4s}
                format={screen.format}
                sourceVideoPath={screen.videoPath}
                outputName={screen.outputName}
                onNewConversion={handleNewConversion}
                onBackToEdit={handleBackToEdit}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
