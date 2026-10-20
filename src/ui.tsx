import {
  useEffect,
  useId,
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
      title={title}
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
      {stateLabel[state] ?? state}
    </span>
  );
}
export function Modal({
  title,
  children,
  onClose,
  wide = false,
  className = '',
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
  className?: string;
}) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [onClose]);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <section
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`modal ${wide ? 'wide' : ''} ${className}`}
      >
        <div className="modal-heading">
          <h2>{title}</h2>
          <button className="icon-button" aria-label="关闭面板" onClick={onClose}>
            <X size={18} />
          </button>
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
    const item = node as ReactElement<{ id?: string; children?: ReactNode }>;
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
      <label htmlFor={id}>{label}</label>
      {Children.map(children, connect)}
      {hint && <small>{hint}</small>}
    </div>
  );
}
