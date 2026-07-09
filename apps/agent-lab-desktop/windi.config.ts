import { defineConfig } from "windicss/helpers";

export default defineConfig({
  preflight: true,
  attributify: false,
  theme: {
    extend: {
      colors: {
        // 和纸白 -> 背景
        paper: {
          DEFAULT: "#f7f5f0",
          deep: "#efece4",
        },
        // 墨黑 -> 主文字
        ink: {
          DEFAULT: "#1a1a1a",
          light: "#3d3d3d",
        },
        // 石炭灰 -> 次要文字
        stone: {
          DEFAULT: "#8c8c8c",
          light: "#b0b0b0",
          dark: "#5c5c5c",
        },
        // 薄雾灰 -> 边框、分隔
        mist: {
          DEFAULT: "#e8e6e1",
          dark: "#d9d6cf",
        },
        // 苔绿 -> 主强调/按钮
        moss: {
          DEFAULT: "#6b7c5a",
          dark: "#5a6a4b",
          light: "#8a9a78",
        },
        // 淡樱 -> 错误/弱提示
        sakura: {
          DEFAULT: "#e8d5d5",
          dark: "#d4bbbb",
        },
      },
      fontFamily: {
        sans: [
          "Inter",
          "Noto Sans JP",
          "Hiragino Sans",
          "Yu Gothic",
          "system-ui",
          "sans-serif",
        ],
      },
      borderRadius: {
        none: "0",
        sm: "2px",
        DEFAULT: "2px",
        md: "4px",
        lg: "6px",
      },
      boxShadow: {
        soft: "0 1px 2px rgba(26, 26, 26, 0.04)",
        card: "0 2px 8px rgba(26, 26, 26, 0.05)",
      },
      lineHeight: {
        relaxed: "1.75",
        loose: "2",
      },
      letterSpacing: {
        wide: "0.05em",
        wider: "0.1em",
      },
    },
  },
  shortcuts: {
    // 日本简约风格常用组合
    "btn-moss":
      "px-6 py-2 bg-moss text-paper rounded-sm transition-colors duration-200 hover:bg-moss-dark focus:outline-none focus:ring-2 focus:ring-moss/30",
    "input-minimal":
      "px-4 py-2 bg-paper border border-mist rounded-sm text-ink placeholder-stone-light focus:outline-none focus:border-stone transition-colors duration-200",
    "card-paper":
      "bg-paper-deep border border-mist rounded-sm shadow-soft",
  },
});
