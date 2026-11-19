import { useEffect, useId, useRef, useState } from 'react';
import { Check, Monitor, Moon, Palette, Plus, Sun } from 'lucide-react';
import { colorThemes, validColor } from './appearance';
import { savePreferences, usePreference } from './preferences';
import { t, useLocale } from './i18n';
import type { Theme } from './theme';
import './appearance.css';

const themeModes = [
  { value: 'system', label: '系统', Icon: Monitor },
  { value: 'light', label: '亮色', Icon: Sun },
  { value: 'dark', label: '暗色', Icon: Moon },
] as const;

export function AppearanceControls({ compact = false }: { compact?: boolean }) {
  useLocale();
  const id = useId();
  const [theme, setTheme] = usePreference('theme');
  const [colorTheme, setColorTheme] = usePreference('colorTheme');
  const [customColor] = usePreference('customColor');
  const [hex, setHex] = useState(customColor);
  const [customOpen, setCustomOpen] = useState(false);
  useEffect(() => setHex(customColor), [customColor]);
  const saveColor = (color: string) => {
    if (validColor(color)) {
      void savePreferences({ colorTheme: 'custom', customColor: color.toLowerCase() }).catch(
        () => {},
      );
    }
  };
  const showCustom = colorTheme === 'custom' && (!compact || customOpen);
  const customEditor = showCustom && (
    <div className="field appearance-custom" id={`${id}-custom`}>
      <label htmlFor={`${id}-color`}>{t('自定义颜色')}</label>
      <div>
        <input
          id={`${id}-color`}
          type="color"
          value={customColor}
          onChange={(event) => saveColor(event.target.value)}
        />
        <input
          type="text"
          aria-label={t('十六进制颜色')}
          value={hex.toUpperCase()}
          maxLength={7}
          spellCheck={false}
          autoComplete="off"
          placeholder="#35B8BD"
          aria-invalid={!validColor(hex)}
          onChange={(event) => setHex(event.target.value)}
          onBlur={() => {
            if (validColor(hex)) saveColor(hex);
            else setHex(customColor);
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault();
              saveColor(hex);
            }
          }}
        />
      </div>
      {!compact && <small>{t('输入 # 和六位十六进制颜色值，按回车或离开输入框应用。')}</small>}
    </div>
  );

  if (compact) {
    return (
      <div className="appearance-quick-controls">
        <span id={`${id}-theme`} className="appearance-quick-label">
          {t('主题')}
        </span>
        <div className="appearance-segments" role="radiogroup" aria-labelledby={`${id}-theme`}>
          {themeModes.map(({ value, label, Icon }) => (
            <label key={value} className="appearance-segment">
              <input
                type="radio"
                name={`${id}-theme-mode`}
                className="sr-only"
                checked={theme === value}
                onChange={() => setTheme(value)}
              />
              <span className="appearance-segment-face">
                <Icon size={14} aria-hidden="true" />
                {t(label)}
              </span>
            </label>
          ))}
        </div>
        <span id={`${id}-palette`} className="appearance-quick-label">
          {t('配色')}
        </span>
        <div className="appearance-quick-colors" role="group" aria-labelledby={`${id}-palette`}>
          {colorThemes.map((color) => {
            const custom = color.id === 'custom';
            return (
              <button
                key={color.id}
                type="button"
                className="appearance-swatch-button"
                aria-label={t(color.name)}
                title={t(color.name)}
                aria-pressed={colorTheme === color.id}
                aria-expanded={custom ? showCustom : undefined}
                aria-controls={custom && showCustom ? `${id}-custom` : undefined}
                onClick={() => {
                  if (colorTheme !== color.id) setColorTheme(color.id);
                  setCustomOpen((open) => custom && (colorTheme !== color.id || !open));
                }}
              >
                {custom ? (
                  <span className="appearance-swatch-custom" aria-hidden="true">
                    <Plus size={12} />
                  </span>
                ) : (
                  <span
                    className={`appearance-swatch${color.id === 'default' ? ' appearance-swatch-default' : ''}`}
                    style={color.id === 'default' ? undefined : { backgroundColor: color.color }}
                    aria-hidden="true"
                  />
                )}
              </button>
            );
          })}
        </div>
        {customEditor}
      </div>
    );
  }

  return (
    <div className="appearance-controls">
      <div className="field appearance-mode">
        <label htmlFor={`${id}-mode`}>{t('界面主题')}</label>
        <select
          id={`${id}-mode`}
          value={theme}
          onChange={(event) => setTheme(event.target.value as Theme)}
        >
          <option value="system">{t('跟随系统')}</option>
          <option value="light">{t('亮色')}</option>
          <option value="dark">{t('暗色')}</option>
        </select>
      </div>
      <fieldset className="appearance-palette">
        <legend>{t('主题配色')}</legend>
        <div className="appearance-colors">
          {colorThemes.map((color) => (
            <button
              key={color.id}
              type="button"
              className="appearance-color"
              aria-pressed={colorTheme === color.id}
              onClick={() => setColorTheme(color.id)}
            >
              <span
                className="appearance-swatch"
                style={{ backgroundColor: color.id === 'custom' ? customColor : color.color }}
                aria-hidden="true"
              />
              <span>{t(color.name)}</span>
              {colorTheme === color.id && <Check size={13} aria-hidden="true" />}
            </button>
          ))}
        </div>
      </fieldset>
      {customEditor}
      <p className="appearance-hint">{t('立即生效并自动保存')}</p>
    </div>
  );
}

export function AppearancePicker() {
  useLocale();
  const [open, setOpen] = useState(false);
  const wrapper = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    panel.current?.querySelector<HTMLInputElement>('input[type="radio"]:checked')?.focus();
    const outside = (event: PointerEvent) => {
      if (!wrapper.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [open]);
  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };
  return (
    <div
      className="appearance-picker"
      ref={wrapper}
      onKeyDown={(event) => {
        if (open && event.key === 'Escape') {
          event.stopPropagation();
          close();
        }
      }}
      onBlur={(event) => {
        if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget as Node))
          setOpen(false);
      }}
    >
      <button
        type="button"
        ref={trigger}
        className="appearance-trigger"
        aria-label={t('外观')}
        title={t('外观')}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
      >
        <Palette size={16} aria-hidden="true" />
      </button>
      {open && (
        <div
          className="appearance-popover"
          ref={panel}
          id={id}
          role="dialog"
          aria-label={t('外观设置')}
        >
          <AppearanceControls compact />
        </div>
      )}
    </div>
  );
}
