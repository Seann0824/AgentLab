import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  isOpen,
  title,
  message,
  confirmText = "确认",
  cancelText = "取消",
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    if (isOpen && !dialog.open) {
      dialog.showModal();
    } else if (!isOpen && dialog.open) {
      dialog.close();
    }
  }, [isOpen]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) return;

    const handleClose = () => {
      // 无论是按 Escape 还是调用 close()，都通知外部关闭
      onCancel();
    };

    dialog.addEventListener("close", handleClose);
    return () => dialog.removeEventListener("close", handleClose);
  }, [onCancel]);

  return (
    <dialog
      ref={dialogRef}
      className="m-auto p-0 bg-transparent backdrop:bg-black/20 backdrop:backdrop-blur-sm"
      onClick={(e) => {
        // 点击 backdrop 关闭
        if (e.target === dialogRef.current) {
          onCancel();
        }
      }}
    >
      <div className="w-full max-w-sm bg-paper rounded-xl shadow-lg border border-mist p-6">
        <h3 className="text-base font-medium text-ink mb-2">{title}</h3>
        <p className="text-sm text-stone mb-6 leading-relaxed">{message}</p>
        <div className="flex justify-end gap-3">
          <button
            onClick={onCancel}
            className="px-4 py-2 text-sm text-stone hover:text-ink transition-colors"
          >
            {cancelText}
          </button>
          <button
            onClick={onConfirm}
            className="px-4 py-2 text-sm rounded-lg bg-red-50 text-red-700 hover:bg-red-100 transition-colors"
          >
            {confirmText}
          </button>
        </div>
      </div>
    </dialog>
  );
}
