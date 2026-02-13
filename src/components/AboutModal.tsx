import { motion, AnimatePresence } from "framer-motion";
import { openUrl } from "@tauri-apps/plugin-opener";

interface AboutModalProps {
  open: boolean;
  onClose: () => void;
}

const LICENSE_ENTRIES = [
  { name: "gifski", license: "AGPL-3.0", url: "https://github.com/ImageOptim/gifski" },
  { name: "Tauri", license: "MIT / Apache-2.0", url: "https://github.com/tauri-apps/tauri" },
  { name: "FFmpeg", license: "LGPL-2.1+", url: "https://ffmpeg.org" },
  { name: "TransNetV2", license: "MIT", url: "https://github.com/soCzech/TransNetV2" },
  { name: "React", license: "MIT", url: "https://react.dev" },
];

function ExternalLink({
  url,
  className,
  children,
}: {
  url: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={() => openUrl(url)}
      className={className}
    >
      {children}
    </button>
  );
}

export default function AboutModal({ open, onClose }: AboutModalProps) {
  return (
    <AnimatePresence>
      {open && (
        <>
          {/* Backdrop */}
          <motion.div
            key="about-backdrop"
            className="fixed inset-0 z-50 bg-black/60"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.2 }}
            onClick={onClose}
          />

          {/* Modal card */}
          <motion.div
            key="about-card"
            className="fixed inset-0 z-50 flex items-center justify-center pointer-events-none"
            initial={{ opacity: 0, scale: 0.92 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.92 }}
            transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
          >
            <div
              className="pointer-events-auto bg-surface rounded-xl border border-border shadow-2xl
                w-[380px] max-h-[80vh] overflow-y-auto"
              onClick={(e) => e.stopPropagation()}
            >
              {/* Header */}
              <div className="flex flex-col items-center pt-6 pb-4 px-6">
                <img
                  src="/app-icon.png"
                  alt="VideoBricks"
                  className="w-16 h-16 rounded-xl mb-3"
                  draggable={false}
                />
                <h2 className="text-lg font-semibold text-text-primary">
                  VideoBricks
                </h2>
                <span className="text-xs text-text-tertiary mt-0.5">
                  Version 1.0.0
                </span>
                <p className="text-xs text-text-secondary mt-2 text-center leading-relaxed max-w-[280px]">
                  Convert videos to high-quality GIFs and optimized MP4s.
                  Multi-segment timeline editing with AI shot detection.
                </p>
              </div>

              {/* Divider */}
              <div className="mx-6 border-t border-border" />

              {/* Donate section */}
              <div className="px-6 py-4 flex flex-col gap-2">
                <span className="text-[11px] uppercase tracking-wider text-text-tertiary font-medium">
                  Support
                </span>

                <ExternalLink
                  url="https://buymeacoffee.com/alonsorobots"
                  className="flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg
                    bg-accent text-white text-sm font-medium
                    hover:bg-accent-hover transition-colors cursor-pointer"
                >
                  <svg className="w-4 h-4" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M4.25 2.5c-1.336 0-2.75 1.164-2.75 3 0 2.15 1.58 4.144
                      3.365 5.682A20.6 20.6 0 008 13.393a20.6 20.6 0 003.135-2.211C12.92
                      9.644 14.5 7.65 14.5 5.5c0-1.836-1.414-3-2.75-3-1.373 0-2.609.986-3.029
                      2.456a.749.749 0 01-1.442 0C6.859 3.486 5.623 2.5 4.25 2.5z" />
                  </svg>
                  Buy me coffee
                </ExternalLink>

                <ExternalLink
                  url="https://github.com/sindresorhus/Gifski"
                  className="flex items-center justify-center gap-2 px-4 py-2 rounded-lg
                    bg-surface-hover text-text-secondary text-xs font-medium
                    hover:bg-border transition-colors cursor-pointer"
                >
                  Inspired by Gifski (Sindre Sorhus)
                </ExternalLink>
              </div>

              {/* Divider */}
              <div className="mx-6 border-t border-border" />

              {/* Licenses */}
              <div className="px-6 py-4">
                <span className="text-[11px] uppercase tracking-wider text-text-tertiary font-medium">
                  Open-Source Licenses
                </span>
                <p className="text-[11px] text-text-tertiary mt-1.5 leading-relaxed">
                  This application is distributed under the{" "}
                  <span className="text-text-secondary font-medium">AGPL-3.0</span>{" "}
                  license. Source code is publicly available.
                </p>

                <div className="mt-3 flex flex-col gap-1">
                  {LICENSE_ENTRIES.map((entry) => (
                    <ExternalLink
                      key={entry.name}
                      url={entry.url}
                      className="flex items-center justify-between py-1.5 px-2 -mx-2 rounded
                        hover:bg-surface-hover transition-colors group cursor-pointer text-left"
                    >
                      <span className="text-xs text-text-secondary group-hover:text-text-primary transition-colors">
                        {entry.name}
                      </span>
                      <span className="text-[10px] text-text-tertiary font-mono">
                        {entry.license}
                      </span>
                    </ExternalLink>
                  ))}
                </div>
              </div>

              {/* Divider */}
              <div className="mx-6 border-t border-border" />

              {/* Vibe coded */}
              <div className="px-6 py-3">
                <p className="text-[10px] text-text-tertiary text-center leading-relaxed italic">
                  Vibe coded with the help of Claude the conqueror.
                </p>
              </div>

              {/* Close button */}
              <div className="px-6 pb-5 pt-1">
                <button
                  onClick={onClose}
                  className="w-full py-2 rounded-lg bg-surface-hover text-text-secondary text-xs
                    font-medium hover:bg-border hover:text-text-primary transition-colors"
                >
                  Close
                </button>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
