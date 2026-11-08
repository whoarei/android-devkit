import { useId } from 'react';
import { appearanceTokens, brandColorMatrix } from './appearance';
import { usePreferenceValue } from './preferences';
import { useResolvedTheme } from './theme';

export function BrandMark() {
  const theme = usePreferenceValue('theme');
  const colorTheme = usePreferenceValue('colorTheme');
  const customColor = usePreferenceValue('customColor');
  const resolved = useResolvedTheme();
  const id = `brand-${useId().replace(/:/g, '')}`;
  const tokens = appearanceTokens({ theme, colorTheme, customColor }, resolved);
  return (
    <svg className="brand-mark" viewBox="0 0 128 128" aria-hidden="true" focusable="false">
      <defs>
        <filter id={id} x="0" y="0" width="100%" height="100%" colorInterpolationFilters="sRGB">
          <feColorMatrix
            values={brandColorMatrix(tokens['--brand-fill'], tokens['--brand-foreground'])}
          />
        </filter>
      </defs>
      <image href="/android-devkit.png" width="128" height="128" filter={`url(#${id})`} />
    </svg>
  );
}
