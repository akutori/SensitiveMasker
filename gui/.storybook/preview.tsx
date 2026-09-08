import type { Preview } from '@storybook/react-vite'
import '../src/index.css'

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
       color: /(background|color)$/i,
       date: /Date$/i,
      },
    },

    a11y: {
      // 'todo' - show a11y violations in the test UI only
      // 'error' - fail CI on a11y violations
      // 'off' - skip a11y checks entirely
      test: 'todo',
      config: {
        rules: [
          {
            // Radix UIのfocus-guard(フォーカストラップ用の空span)がaria-hidden化される
            // 既知のバグ。実コンテンツを持たないsentinel要素のため無効化する。
            id: 'aria-hidden-focus',
            enabled: false,
          },
          {
            // 要素の重なりにより背景色を判定できずInconclusiveになるケースがあるため、
            // 確認済みの要素はdata-a11y-verified-contrast属性(子孫要素含む)で個別に除外する。
            // textarea.ime-text-areaはMonaco Editor内部のIME合成用要素で属性を付与できないため
            // クラス名で除外する。
            id: 'color-contrast',
            selector:
              '*:not([data-a11y-verified-contrast]):not(textarea.ime-text-area):not([data-a11y-verified-contrast] *)',
          },
        ],
      },
    }
  },
};

export default preview;