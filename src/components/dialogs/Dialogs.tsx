import { useEffect, useRef, useState } from "react";
import { useUiStore } from "../../state/uiStore";

export function InputDialog() {
  const dialog = useUiStore((s) => s.inputDialog);
  const close = useUiStore((s) => s.closeInput);
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (dialog) {
      setValue(dialog.value);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [dialog]);

  if (!dialog) return null;

  const submit = () => {
    close();
    dialog.onOk(value);
  };

  return (
    <div className="palette-overlay" onMouseDown={close}>
      <div className="input-dialog" onMouseDown={(e) => e.stopPropagation()} style={{ top: 0 }}>
        <div className="input-dialog-title">{dialog.title}</div>
        <input
          ref={inputRef}
          className="palette-input"
          value={value}
          placeholder={dialog.placeholder}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              submit();
            } else if (e.key === "Escape") {
              e.preventDefault();
              close();
            }
          }}
          spellCheck={false}
        />
      </div>
    </div>
  );
}

export function ConfirmDialog() {
  const dialog = useUiStore((s) => s.confirmDialog);
  const close = useUiStore((s) => s.closeConfirm);

  if (!dialog) return null;

  return (
    <div className="modal-overlay" onMouseDown={close}>
      <div className="confirm-dialog" onMouseDown={(e) => e.stopPropagation()}>
        <div className="confirm-title">{dialog.title}</div>
        <div className="confirm-message">{dialog.message}</div>
        <div className="confirm-actions">
          <button className="btn-secondary" onClick={close}>
            Cancel
          </button>
          <button
            className="btn-primary"
            onClick={() => {
              close();
              dialog.onOk?.();
            }}
          >
            {dialog.okLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
