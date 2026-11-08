import { useEffect, useId, useRef, useState } from 'react';
import { Check, ChevronDown, Palette, X } from 'lucide-react';
import { colorThemes, validColor } from './appearance';
import { savePreferences, usePreference } from './preferences';
import { t, useLocale } from './i18n';
import type { Theme } from './theme';
import './appearance.css';

export function AppearanceControls() {
  useLocale();
  const id = useId();
  const [theme, setTheme] = usePreference('theme');
  const [colorTheme, setColorTheme] = usePreference('colorTheme');
  const [customColor] = usePreference('customColor');
  const [hex, setHex] = useState(customColor);
  useEffect(() => setHex(customColor), [customColor]);
  const saveColor = (color: string) => {
    if (validColor(color)) {
      void savePreferences({ colorTheme: 'custom', customColor: color.toLowerCase() }).catch(
        () => {},
      );
    }
  };
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
      {colorTheme === 'custom' && (
        <div className="field appearance-custom">
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
          <small>{t('输入 # 和六位十六进制颜色值，按回车或离开输入框应用。')}</small>
        </div>
      )}
      <p className="appearance-hint">{t('立即生效并自动保存')}</p>
    </div>
  );
}

export function AppearancePicker() {
  useLocale();
  const [open, setOpen] = useState(false);
  const [colorTheme] = usePreference('colorTheme');
  const [customColor] = usePreference('customColor');
  const wrapper = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    panel.current?.querySelector<HTMLSelectElement>('select')?.focus();
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
  const color =
    colorTheme === 'custom'
      ? customColor
      : colorThemes.find((item) => item.id === colorTheme)!.color;
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
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen(!open)}
      >
        <Palette size={14} aria-hidden="true" />
        <span>{t('外观')}</span>
        <span className="appearance-swatch" style={{ backgroundColor: color }} aria-hidden="true" />
        <ChevronDown size={12} aria-hidden="true" />
      </button>
      {open && (
        <div
          className="appearance-popover"
          ref={panel}
          id={id}
          role="dialog"
          aria-label={t('外观设置')}
        >
          <div className="appearance-heading">
            <strong>{t('外观设置')}</strong>
            <button
              type="button"
              className="icon-button"
              aria-label={t('关闭外观设置')}
              onClick={close}
            >
              <X size={16} />
            </button>
          </div>
          <AppearanceControls />
        </div>
      )}
    </div>
  );
}
