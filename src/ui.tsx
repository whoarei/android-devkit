import { t } from './i18n';
import {
  useEffect,
  useId,
  useRef,
  Children,
  isValidElement,
  cloneElement,
  type ReactNode,
  type ReactElement,
} from 'react';
import { X } from 'lucide-react';
import { stateLabel } from './api';
export function Button({
  children,
  onClick,
  primary = false,
  danger = false,
  disabled = false,
  loading = false,
  title,
}: {
  children: ReactNode;
  onClick?: () => void;
  primary?: boolean;
  danger?: boolean;
  disabled?: boolean;
  loading?: boolean;
  title?: string;
}) {
  return (
    <button
      title={title ? t(title) : undefined}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={`button ${primary ? 'primary' : ''} ${danger ? 'danger' : ''}`}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
export function Badge({ state }: { state: string }) {
  return (
    <span className={`badge ${state}`}>
      <i />
      {t(stateLabel[state] ?? state)}
    </span>
  );
}
export function Modal({
  title,
  children,
  onClose,
  wide = false,
  className = '',
  hidden = false,
  dismissible = true,
  trapFocus = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
  className?: string;
  hidden?: boolean;
  dismissible?: boolean;
  trapFocus?: boolean;
}) {
  const dialog = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!trapFocus || hidden || !dialog.current) return;
    const panel = dialog.current;
    const previous = document.activeElement;
    const controls = () =>
      [
        ...panel.querySelectorAll<HTMLElement>(
          'button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),summary,a[href]',
        ),
      ].filter((node) => node.getClientRects().length > 0);
    controls()[0]?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const nodes = controls();
      if (event.shiftKey && document.activeElement === nodes[0]) {
        event.preventDefault();
        nodes.at(-1)?.focus();
      } else if (!event.shiftKey && document.activeElement === nodes.at(-1)) {
        event.preventDefault();
        nodes[0]?.focus();
      }
    };
    panel.addEventListener('keydown', trap);
    return () => {
      panel.removeEventListener('keydown', trap);
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [trapFocus, hidden]);
  useEffect(() => {
    if (hidden || !dismissible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose, hidden, dismissible]);
  return (
    <div
      className="modal-backdrop"
      hidden={hidden}
      onMouseDown={(e) => {
        if (dismissible && e.target === e.currentTarget) onClose();
      }}
    >
      <section
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-label={t(title)}
        className={`modal ${wide ? 'wide' : ''} ${className}`}
      >
        <div className="modal-heading">
          <h2>{t(title)}</h2>
          {dismissible && (
            <button className="icon-button" aria-label={t('关闭面板')} onClick={onClose}>
              <X size={18} />
            </button>
          )}
        </div>
        {children}
      </section>
    </div>
  );
}
export function Field({
  label,
  children,
  hint,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
}) {
  const id = useId();
  let connected = false;
  const connect = (node: ReactNode): ReactNode => {
    if (!isValidElement(node)) return node;
    const item = node as ReactElement<{
      id?: string;
      children?: ReactNode;
    }>;
    if (typeof item.type === 'string' && ['input', 'select', 'textarea'].includes(item.type)) {
      if (connected) return item;
      connected = true;
      return cloneElement(item, { id });
    }
    return item.props.children
      ? cloneElement(item, { children: Children.map(item.props.children, connect) })
      : item;
  };
  return (
    <div className="field">
      <label htmlFor={id}>{t(label)}</label>
      {Children.map(children, connect)}
      {hint && <small>{t(hint)}</small>}
    </div>
  );
}
