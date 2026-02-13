import { useState, useCallback, useRef, useEffect } from "react";

interface EditableValueProps {
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  /** Format the value for display (default: toString) */
  format?: (value: number) => string;
  /** Parse the display string back to a number (default: parseFloat) */
  parse?: (str: string) => number;
  className?: string;
}

export default function EditableValue({
  value,
  onChange,
  min,
  max,
  step,
  format,
  parse,
  className = "",
}: EditableValueProps) {
  const [editing, setEditing] = useState(false);
  const [inputValue, setInputValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLSpanElement>(null);

  const displayValue = format ? format(value) : String(value);

  useEffect(() => {
    if (editing && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editing]);

  const commit = useCallback(() => {
    const parser = parse || parseFloat;
    let num = parser(inputValue);
    if (isNaN(num)) {
      setEditing(false);
      return;
    }
    if (min !== undefined) num = Math.max(min, num);
    if (max !== undefined) num = Math.min(max, num);
    if (step !== undefined) {
      num = Math.round(num / step) * step;
      // Fix floating point
      const decimals = (step.toString().split(".")[1] || "").length;
      num = parseFloat(num.toFixed(decimals));
    }
    onChange(num);
    setEditing(false);
  }, [inputValue, min, max, step, onChange, parse]);

  const handleClick = useCallback(() => {
    setInputValue(String(value));
    setEditing(true);
  }, [value]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        commit();
      } else if (e.key === "Escape") {
        setEditing(false);
      }
    },
    [commit]
  );

  // Use a single inline-block container that never changes size.
  // The span always renders (for measurement), and the input overlays it when editing.
  return (
    <span
      ref={containerRef}
      className={`relative inline-block cursor-pointer ${className}`}
      onClick={!editing ? handleClick : undefined}
      title={!editing ? "Click to edit" : undefined}
    >
      {/* Always render the display text to maintain layout size */}
      <span className={editing ? "invisible" : ""}>{displayValue}</span>

      {/* Input overlays the display text when editing */}
      {editing && (
        <input
          ref={inputRef}
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onBlur={commit}
          onKeyDown={handleKeyDown}
          className="absolute inset-0 w-full bg-surface-raised border border-accent rounded
            text-xs text-text-primary text-center tabular-nums focus:outline-none"
          style={{ padding: "0 2px" }}
        />
      )}
    </span>
  );
}
