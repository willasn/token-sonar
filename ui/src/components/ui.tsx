/* TokenSonar · 通用 UI 层：Modal / Confirm / Toast / 全局悬浮提示
 *
 * 对应旧的 modal.js：DOM 结构与 class 完全沿用（modal-root / toast-root / tl-tooltip），
 * 因此既有 CSS 无需改动。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Icon } from "../lib/icons";
import { t } from "../lib/i18n";

export type ToastType = "info" | "ok" | "error" | "warn";

export interface ModalButton {
  label: string;
  kind?: string;
  onClick?: () => void | Promise<void>;
}

export interface ModalOptions {
  title: string;
  subtitle?: string;
  body: ReactNode;
  wide?: boolean;
  footNote?: ReactNode;
  buttons?: ModalButton[];
  dismissible?: boolean;
  onClose?: () => void;
}

export interface ConfirmOptions {
  title?: string;
  subtitle?: string;
  message: string;
  okLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
}

interface UiContextValue {
  toast: (message: string, type?: ToastType) => void;
  confirm: (opts: ConfirmOptions) => Promise<boolean>;
  openModal: (opts: ModalOptions) => void;
  closeModal: () => void;
}

const UiContext = createContext<UiContextValue | null>(null);

interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
  out: boolean;
}

let uid = 0;

export function UiProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [modal, setModal] = useState<ModalOptions | null>(null);
  // 用于 confirm 的 resolver
  const confirmResolver = useRef<((v: boolean) => void) | null>(null);

  const toast = useCallback((message: string, type: ToastType = "info") => {
    const id = ++uid;
    setToasts((prev) => prev.concat([{ id, message, type, out: false }]));
    const life = type === "error" ? 4200 : 2600;
    window.setTimeout(() => {
      setToasts((prev) => prev.map((x) => (x.id === id ? { ...x, out: true } : x)));
      window.setTimeout(() => {
        setToasts((prev) => prev.filter((x) => x.id !== id));
      }, 220);
    }, life);
  }, []);

  const closeModal = useCallback(() => {
    setModal((prev) => {
      if (prev && prev.onClose) prev.onClose();
      return null;
    });
  }, []);

  const openModal = useCallback((opts: ModalOptions) => {
    setModal((prev) => {
      if (prev && prev.onClose) prev.onClose();
      return opts;
    });
  }, []);

  const confirm = useCallback(
    (opts: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        let done = false;
        const finish = (v: boolean) => {
          if (done) return;
          done = true;
          confirmResolver.current = null;
          setModal(null);
          resolve(v);
        };
        confirmResolver.current = finish;
        setModal({
          title: opts.title || t("common.confirmTitle"),
          subtitle: opts.subtitle || "",
          body: (
            <div style={{ fontSize: 13, color: "var(--text-dim)", lineHeight: 1.7 }}>{opts.message}</div>
          ),
          wide: false,
          buttons: [
            { label: opts.cancelLabel || t("common.cancel"), onClick: () => finish(false) },
            {
              label: opts.okLabel || t("common.ok"),
              kind: opts.danger ? "btn-danger" : "btn-primary",
              onClick: () => finish(true),
            },
          ],
        });
      }),
    []
  );

  // Esc 关闭
  useEffect(() => {
    if (!modal) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeModal();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [modal, closeModal]);

  const value: UiContextValue = { toast, confirm, openModal, closeModal };

  return (
    <UiContext.Provider value={value}>
      {children}

      <div className={"modal-root" + (modal ? " is-open" : "")} id="modalRoot">
        {modal && (
          <>
            <div
              className="modal-scrim"
              onClick={() => {
                if (modal.dismissible !== false) closeModal();
              }}
            />
            <div className={"modal" + (modal.wide ? " is-wide" : "")}>
              <div className="modal-head">
                <div>
                  <h3>{modal.title}</h3>
                  {modal.subtitle ? <p>{modal.subtitle}</p> : null}
                </div>
                <button className="modal-close" title={t("common.close")} onClick={closeModal}>
                  <Icon name="close" />
                </button>
              </div>
              <div className="modal-body">{modal.body}</div>
              <div className="modal-foot">
                {modal.footNote}
                <div className="spacer" />
                {(modal.buttons || []).map((b, i) => (
                  <button key={i} className={"btn " + (b.kind || "")} onClick={() => b.onClick && b.onClick()}>
                    {b.label}
                  </button>
                ))}
              </div>
            </div>
          </>
        )}
      </div>

      <div className="toast-root" id="toastRoot">
        {toasts.map((x) => (
          <div key={x.id} className={"toast is-" + x.type + (x.out ? " is-out" : "")}>
            <span className="dot" />
            <span>{x.message}</span>
          </div>
        ))}
      </div>

      <GlobalTooltip />
    </UiContext.Provider>
  );
}

export function useUi(): UiContextValue {
  const ctx = useContext(UiContext);
  if (!ctx) throw new Error("useUi 必须在 UiProvider 内使用");
  return ctx;
}

/** 全局悬浮提示：任何带 data-tip 的元素，鼠标悬浮时用浮层展示完整内容 */
function GlobalTooltip() {
  const tipRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef(false);

  useEffect(() => {
    const tip = tipRef.current;
    if (!tip) return;

    const place = (x: number, y: number) => {
      const pad = 12;
      const w = tip.offsetWidth;
      const h = tip.offsetHeight;
      let left = x + 14;
      let top = y + 18;
      if (left + w > window.innerWidth - pad) left = x - w - 14;
      if (top + h > window.innerHeight - pad) top = y - h - 18;
      tip.style.left = Math.max(pad, left) + "px";
      tip.style.top = Math.max(pad, top) + "px";
    };

    const open = (el: Element, x: number, y: number) => {
      tip.textContent = el.getAttribute("data-tip");
      tip.classList.add("is-open");
      activeRef.current = true;
      place(x, y);
    };

    const close = () => {
      if (!activeRef.current) return;
      tip.classList.remove("is-open");
      activeRef.current = false;
    };

    const target = (node: EventTarget | null): Element | null => {
      const el = node as Element | null;
      return el && el.closest ? el.closest("[data-tip]") : null;
    };

    const onOver = (e: MouseEvent) => {
      const el = target(e.target);
      if (el) open(el, e.clientX, e.clientY);
      else close();
    };
    const onMove = (e: MouseEvent) => {
      if (activeRef.current) place(e.clientX, e.clientY);
    };
    const onOut = (e: MouseEvent) => {
      const el = target(e.target);
      if (!el) return;
      if (e.relatedTarget && el.contains(e.relatedTarget as Node)) return;
      close();
    };

    document.addEventListener("mouseover", onOver);
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseout", onOut);
    window.addEventListener("scroll", close, true);
    return () => {
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseout", onOut);
      window.removeEventListener("scroll", close, true);
    };
  }, []);

  return <div className="tl-tooltip" ref={tipRef} />;
}
