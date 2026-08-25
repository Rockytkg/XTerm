import { defineConfig, presetUno } from "unocss";

export default defineConfig({
  presets: [presetUno()],
  content: {
    pipeline: {
      include: [/\.(vue|html)($|\?)/, "src/**/*.{js,vue}", "index.html"],
      exclude: ["dist/**", "src-tauri/**", "node_modules/**"],
    },
  },
  theme: {
    colors: {
      bg: {
        primary: "var(--bg-primary)",
        secondary: "var(--bg-secondary)",
        tertiary: "var(--bg-tertiary)",
        terminal: "var(--bg-terminal)",
      },
      text: {
        primary: "var(--text-primary)",
        secondary: "var(--text-secondary)",
        tertiary: "var(--text-tertiary)",
        terminal: "var(--text-terminal)",
      },
      accent: {
        DEFAULT: "var(--accent)",
        hover: "var(--accent-hover)",
        active: "var(--accent-active)",
        light: "var(--accent-light)",
      },
      border: {
        DEFAULT: "var(--border)",
        light: "var(--border-light)",
      },
      success: {
        DEFAULT: "var(--success)",
        bg: "var(--success-bg)",
      },
      warning: {
        DEFAULT: "var(--warning)",
        bg: "var(--warning-bg)",
      },
      danger: {
        DEFAULT: "var(--danger)",
        bg: "var(--danger-bg)",
      },
      info: {
        DEFAULT: "var(--info)",
        bg: "var(--info-bg)",
      },
    },
    fontFamily: {
      sans: "var(--font-sans)",
    },
    boxShadow: {
      sm: "var(--shadow-sm)",
      md: "var(--shadow-md)",
      lg: "var(--shadow-lg)",
      xl: "var(--shadow-xl)",
    },
  },
  shortcuts: {
    // Status dots
    "ui-status-dot": "inline-block w-[6px] h-[6px] rounded-full flex-shrink-0",
    "ui-status-dot-online":
      "ui-status-dot bg-[var(--success)] shadow-[0_0_5px_var(--success-glow)]",
    "ui-status-dot-warning":
      "ui-status-dot bg-[var(--warning)] shadow-[0_0_5px_var(--warning-glow)]",
    "ui-status-dot-offline": "ui-status-dot bg-[var(--text-tertiary)]",
    // Nav rail
    "ui-nav-item":
      "relative flex items-center gap-[8px] h-[36px] px-[7px] border-none rounded-[8px] bg-transparent text-[var(--text-tertiary)] transition-[background-color,color] duration-150 ease-[var(--ease-default)] whitespace-nowrap overflow-hidden cursor-pointer hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]",
    "ui-nav-item-active": "bg-[var(--accent-light)]! text-[var(--accent)]!",
    // Workspace session tabs. Window dragging is handled by
    // `data-tauri-drag-region` in the template; `-webkit-app-region` is
    // Electron-only and has no effect under Tauri.
    "ui-session-tab":
      "inline-flex items-center gap-[7px] h-[30px] pl-[9px] pr-[7px] border border-transparent rounded-[7px] bg-transparent text-[var(--text-tertiary)] text-[0.8571em] whitespace-nowrap flex-shrink-0 cursor-grab outline-none transition-[background-color,color,border-color,box-shadow] duration-150 ease-[var(--ease-default)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-secondary)]",
    "ui-session-tab-active":
      "border-[var(--border)]! bg-[var(--bg-primary)]! text-[var(--text-primary)]! font-500! shadow-[0_1px_3px_oklch(0%_0_0deg_/_8%)]!",
    // Settings sidebar nav
    "ui-settings-nav-link":
      "flex items-center gap-[8px] h-[34px] px-[10px] border-none rounded-[8px] bg-transparent text-[var(--text-secondary)] text-[0.8929em] text-left cursor-pointer transition-[background-color,color] duration-150 ease-[var(--ease-default)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]",
    "ui-settings-nav-link-active":
      "bg-[var(--accent-light)]! text-[var(--accent)]! font-500! hover:bg-[var(--accent-light)]! hover:text-[var(--accent)]!",
    "ui-button":
      "inline-flex items-center justify-center gap-[6px] h-[38px] rounded-[8px] px-[20px] text-[0.9286em] font-500 transition-[background-color,color,border-color,box-shadow,transform,opacity] duration-200 ease-[var(--ease-default)] focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)] disabled:pointer-events-none disabled:opacity-40",
    "ui-button-primary":
      "ui-button border-0 bg-[var(--accent)] text-white shadow-[0_2px_8px_var(--accent-shadow)] hover:bg-[var(--accent-hover)] hover:-translate-y-[1px] active:bg-[var(--accent-active)] active:translate-y-0",
    "ui-button-secondary":
      "ui-button border border-[var(--border)] bg-[var(--bg-tertiary)] text-[var(--text-primary)] hover:bg-[var(--bg-secondary)] active:bg-[var(--bg-tertiary)]",
    "ui-fill-inline": "[inline-size:-webkit-fill-available] [inline-size:stretch]",
    "ui-fill-block": "[block-size:-webkit-fill-available] [block-size:stretch]",
    "ui-input":
      "h-[40px] rounded-[8px] border-[1.5px] border-transparent bg-[var(--bg-tertiary)] px-[14px] text-[0.9286em] text-[var(--text-primary)] outline-none transition-[background-color,border-color,box-shadow,opacity] duration-200 ease-[var(--ease-default)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--accent)] focus:bg-[var(--bg-secondary)] focus:shadow-[var(--focus-ring)] disabled:opacity-50",
    "ui-input-inline":
      "h-[32px] w-[96px] shrink-0 rounded-[7px] border-[1.5px] border-transparent bg-[var(--bg-tertiary)] px-[10px] text-[0.9286em] text-[var(--text-primary)] outline-none transition-[background-color,border-color,box-shadow,opacity] duration-200 ease-[var(--ease-default)] focus:border-[var(--accent)] focus:bg-[var(--bg-secondary)] focus:shadow-[var(--focus-ring)]",
    "ui-overline":
      "text-[0.7857em] font-500 uppercase tracking-[0.7px] text-[var(--text-tertiary)]",
    "ui-icon-button":
      "inline-flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-[8px] text-[var(--text-secondary)] transition-[background-color,color,box-shadow,transform] duration-200 ease-[var(--ease-default)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)] focus-visible:outline-none focus-visible:shadow-[var(--focus-ring)]",
    // Shared layout primitives
    "ui-page-header":
      "shrink-0 flex items-center justify-between gap-[16px] border-b border-border-light bg-bg-secondary px-[24px] pb-[16px] pt-[20px]",
    "ui-page-header-main": "flex items-center gap-[12px]",
    "ui-page-title": "m-0 text-[1.0714em] font-600 text-text-primary",
    "ui-page-desc": "m-0 mt-[2px] text-[0.8571em] text-text-secondary",
    "ui-empty-state": "flex flex-col items-center justify-center text-center text-text-tertiary",
    "ui-row-action":
      "inline-flex h-[28px] min-w-[28px] shrink-0 items-center justify-center gap-[5px] whitespace-nowrap rounded-[7px] border border-transparent bg-transparent px-[7px] text-text-tertiary transition-[background-color,color,border-color] duration-150 hover:border-border hover:bg-bg-tertiary hover:text-text-primary",
    "ui-row-action-danger": "hover:border-transparent hover:bg-danger-bg hover:text-danger",
    // Credentials view
    "cred-root": "flex ui-fill-block flex-col overflow-hidden bg-bg-primary",
    "cred-content": "flex-1 min-h-0 overflow-hidden grid grid-cols-[minmax(0,1fr)]",
    "cred-list-pane": "min-h-0 min-w-0 flex flex-col overflow-hidden",
    "cred-file-btn":
      "inline-flex h-[24px] items-center gap-[5px] rounded-[6px] border border-border-light bg-bg-primary px-[9px] text-[0.7857em] text-text-secondary transition-[background-color,color,border-color] duration-150 hover:border-border hover:bg-bg-tertiary hover:text-text-primary",
    "cred-list-error":
      "mt-[12px] shrink-0 rounded-[8px] border border-danger-bg bg-danger-bg px-[12px] py-[10px] text-[0.8214em] leading-[1.4] text-danger mx-[16px]",
    "cred-list":
      "flex-1 min-h-0 overflow-y-auto p-[16px] grid grid-cols-[repeat(auto-fit,minmax(min(100%,300px),1fr))] gap-[12px] content-start",
    "cred-card":
      "grid grid-cols-[minmax(0,1fr)] gap-0 rounded-[8px] border border-border-light bg-bg-secondary p-[12px] cursor-grab transition-[border-color,box-shadow] duration-150 hover:border-accent hover:shadow-[var(--focus-ring)]",
    "cred-card-main": "min-w-0 flex items-start gap-[10px]",
    "cred-card-icon":
      "mt-[1px] grid h-[32px] w-[32px] shrink-0 place-items-center rounded-[8px] [&>svg]:block",
    "cred-icon-key": "bg-accent-light text-accent",
    "cred-icon-pw": "bg-success-bg text-success",
    "cred-card-name": "min-w-0 truncate text-[0.9286em] font-600 text-text-primary",
    "cred-card-badge": "shrink-0 rounded-[4px] px-[6px] py-[1px] text-[0.7143em] font-700",
    "badge-key": "bg-accent-light text-accent",
    "badge-pw": "bg-success-bg text-success",
    "cred-card-actions":
      "grid grid-flow-col auto-cols-[28px] gap-[2px] justify-items-end items-center",
    // SFTP panel
    "sftp-root":
      "relative ui-fill-block min-h-0 ui-fill-inline overflow-hidden bg-bg-primary p-[10px] text-text-primary grid gap-[8px] [grid-template-rows:var(--sftp-root-rows)]",
    "sftp-toolbar":
      "min-h-[52px] flex items-center gap-[14px] border-b border-border-light bg-bg-secondary px-[16px]",
    "sftp-title": "min-w-0 flex items-center gap-[10px]",
    "sftp-title-icon":
      "inline-flex h-[30px] w-[30px] items-center justify-center rounded-[8px] bg-accent-light text-accent",
    "sftp-toolbar-actions": "ml-auto flex items-center gap-[8px]",
    "sftp-button":
      "min-w-[112px] h-[30px] inline-flex items-center justify-center gap-[7px] rounded-[7px] border border-border bg-bg-primary px-[11px] text-[0.8214em] text-text-secondary transition-[background-color,color,border-color] duration-150 hover:border-accent hover:bg-accent-light hover:text-accent disabled:cursor-not-allowed disabled:opacity-45",
    "sftp-button-primary":
      "border-[color-mix(in_oklch,var(--accent)_62%,var(--border))] bg-accent-light text-accent hover:bg-accent hover:text-white",
    "sftp-icon-button":
      "inline-flex h-[30px] w-[30px] items-center justify-center rounded-[7px] bg-transparent text-text-tertiary transition-[background-color,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-45",
    "sftp-pathbar":
      "min-h-[44px] grid grid-cols-[auto_minmax(0,1fr)_minmax(180px,300px)] items-center gap-[8px] border-b border-border-light bg-[linear-gradient(180deg,color-mix(in_oklch,var(--bg-secondary)_88%,var(--accent-light)),var(--bg-secondary))] px-[12px] py-[7px]",
    "sftp-path-shell":
      "min-w-0 h-[32px] grid grid-cols-[minmax(0,1fr)_auto] items-center gap-[8px] overflow-hidden rounded-[7px] border-0 bg-bg-primary p-[2px] shadow-[inset_0_1px_0_oklch(1_0_0/0.03)] cursor-text transition-[box-shadow,background-color] duration-150 hover:shadow-[0_0_0_2px_var(--accent-glow),inset_0_1px_0_oklch(1_0_0/0.03)]",
    "sftp-crumbs": "min-w-0 flex items-center gap-[2px] overflow-hidden",
    "sftp-crumb":
      "min-w-0 h-[28px] truncate rounded-[6px] bg-transparent px-[8px] text-[0.8214em] text-text-secondary transition-[background-color,color] duration-150 hover:bg-accent-light hover:text-accent",
    "sftp-path-edit": "min-w-0 min-h-0 h-full col-[1/-1]",
    "sftp-path-text-button":
      "h-[28px] rounded-[6px] bg-transparent px-[8px] text-[0.7857em] text-text-tertiary transition-[background-color,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary",
    "sftp-search":
      "min-w-0 h-[30px] flex items-center gap-[7px] rounded-[7px] border border-border-light bg-bg-secondary px-[9px] text-text-tertiary",
    "sftp-inline-error":
      "absolute left-[12px] right-[12px] top-[104px] z-[5] m-0 min-h-[34px] flex items-center gap-[8px] rounded-[7px] border border-[color-mix(in_oklch,var(--danger)_35%,transparent)] bg-danger-bg px-[9px] py-[7px] text-[0.7857em] text-danger",
    "sftp-browser":
      "relative ui-fill-block min-h-0 overflow-auto border border-border-light bg-bg-secondary cursor-default select-none [contain:layout_paint]",
    "sftp-table":
      "[inline-size:var(--sftp-table-inline-size)] table-fixed border-collapse bg-bg-secondary",
    "sftp-table-head": "bg-bg-secondary text-[0.75em] text-text-tertiary",
    "sftp-row":
      "h-[34px] border-b border-border-light bg-transparent text-left text-text-secondary cursor-default select-none transition-[background-color,color,outline-color] duration-150 hover:bg-bg-tertiary hover:text-text-primary",
    "sftp-row-selected": "bg-accent-light text-accent",
    "sftp-row-drop-target":
      "bg-accent-light outline outline-[2px] outline-accent outline-offset-[-2px]",
    "sftp-row-editing": "cursor-default",
    "sftp-name-cell": "min-w-0 ui-fill-inline flex items-center gap-[8px]",
    "sftp-inline-name-input":
      "min-w-[120px] [inline-size:min(360px,62cqi)] h-[24px] rounded-[4px] border border-accent bg-bg-primary px-[5px] text-[0.8214em] text-text-primary shadow-[0_0_0_2px_var(--accent-glow)] outline-none cursor-text select-text",
    "sftp-state":
      "min-h-[190px] flex flex-col items-center justify-center gap-[8px] text-[0.8571em] text-text-tertiary",
    "sftp-queue":
      "min-h-0 overflow-hidden border-t border-border-light bg-bg-secondary grid grid-rows-[auto_minmax(0,1fr)]",
    "sftp-queue-heading":
      "min-h-[40px] flex items-center gap-[10px] border-b border-border-light px-[12px]",
    "sftp-queue-toggle":
      "inline-flex h-[24px] w-[24px] shrink-0 items-center justify-center rounded-[6px] bg-bg-primary text-text-tertiary transition-[background-color,color] duration-150 hover:bg-accent-light hover:text-accent",
    "sftp-text-button":
      "ml-auto h-[24px] rounded-[6px] bg-transparent px-[8px] text-[0.7857em] text-text-tertiary transition-[background-color,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-45",
    "sftp-queue-empty":
      "min-h-[72px] flex items-center justify-center gap-[7px] text-[0.7857em] text-text-tertiary",
    "sftp-queue-list": "overflow-y-auto px-[8px] pb-[8px] pt-[5px]",
    "sftp-queue-item":
      "grid gap-[5px] border-b border-border-light px-[4px] py-[7px] text-text-secondary",
    "sftp-queue-item-done": "text-success",
    "sftp-queue-item-failed": "text-danger",
    "sftp-queue-row": "min-w-0 grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[7px]",
    "sftp-queue-actions": "ml-auto flex min-w-[48px] shrink-0 items-center justify-end gap-[5px]",
    "sftp-queue-meta":
      "min-w-0 flex items-center justify-between gap-[7px] text-[0.75em] text-text-tertiary",
    "sftp-queue-name-wrap": "min-w-0 inline-flex items-center gap-[6px]",
    "sftp-queue-name": "min-w-0 truncate text-[0.8214em] text-text-primary",
    "sftp-queue-progress-pct":
      "min-w-[3.2em] shrink-0 text-right [font-variant-numeric:tabular-nums]",
    "sftp-queue-close":
      "ml-auto inline-flex h-[20px] w-[20px] items-center justify-center rounded-[5px] bg-transparent p-0 text-text-tertiary leading-none transition-[background-color,color] duration-150 hover:bg-danger-bg hover:text-danger",
    "sftp-progress-track": "h-[4px] overflow-hidden rounded-full bg-bg-tertiary",
    "sftp-progress-bar":
      "ui-fill-block rounded-[inherit] bg-accent transition-[width] duration-150 ease-linear",
    "sftp-delete-list": "grid max-h-[122px] gap-[4px] overflow-auto",
    // Connection dialog system
    "conn-dialog-overlay": {
      "z-index": "30",
      background: "var(--overlay-bg)",
      // WKWebView / old WebKitGTK need the prefixed blur; unprefixed follows.
      "-webkit-backdrop-filter": "blur(4px)",
      "backdrop-filter": "blur(4px)",
    },
    "conn-dialog":
      "grid grid-rows-[auto_minmax(0,1fr)_auto] [inline-size:min(560px,calc(var(--viewport-min-inline)-var(--space-6)))] [max-block-size:calc(var(--viewport-min-block)-var(--space-6))] overflow-hidden rounded-[14px] border border-border bg-bg-secondary shadow-[0_24px_70px_oklch(0_0_0/0.2),0_6px_18px_oklch(0_0_0/0.12)]",
    "conn-dialog-header":
      "flex items-start gap-[12px] border-b border-border-light px-[20px] pb-[15px] pt-[18px]",
    "conn-dialog-header-icon":
      "mt-[1px] flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-[8px] bg-accent-light text-accent",
    "conn-dialog-title": "m-0 text-[1em] font-600 leading-[1.4]",
    "conn-dialog-desc": "m-0 mt-[2px] text-[0.8214em] leading-[1.45] text-text-secondary",
    "conn-dialog-body":
      "min-h-0 flex flex-col gap-[12px] overflow-y-auto px-[20px] pt-[18px] pb-[16px] [scroll-padding-block-end:var(--space-4)]",
    "conn-dialog-footer":
      "flex items-center justify-end gap-[8px] border-t border-border-light px-[20px] pb-[16px] pt-[12px]",
    "conn-field-group": "min-w-0 flex flex-col gap-[6px]",
    "conn-field-label": "text-[0.8214em] font-600 text-text-secondary",
    "conn-field-heading": "flex items-center justify-between gap-[10px]",
    "conn-field-hint": "mt-[2px] block text-[0.7857em] leading-[1.4] text-text-tertiary",
    "conn-field-error": "text-[0.7857em] leading-[1.35] text-danger",
    "conn-input-error": "border-danger! shadow-[0_0_0_3px_var(--danger-bg)]!",
    "conn-textarea": "min-h-[70px] resize-y text-[0.7857em]",
    "conn-inline-control": "grid grid-cols-[minmax(0,1fr)_34px] gap-[8px]",
    "conn-auth-line-grid": "grid grid-cols-[repeat(2,minmax(0,1fr))] gap-[10px]",
    "conn-serial-line-grid": "grid grid-cols-[repeat(2,minmax(0,1fr))] gap-[10px]",
    "conn-toggle-row": "flex items-center justify-between gap-[14px] p-0",
    "conn-advanced": "mt-[2px] border-t border-border-light pt-[10px]",
    "conn-advanced-summary":
      "ui-fill-inline min-h-[32px] flex items-center justify-between gap-[10px] border-0 bg-transparent p-0 text-left text-[0.8214em] font-600 text-text-secondary",
    "conn-advanced-summary-label": "min-w-0 flex items-center gap-[7px]",
    "conn-advanced-content": "grid gap-[10px] pt-[8px]",
    "conn-jump-summary-row":
      "min-w-0 flex items-center justify-between gap-[12px] rounded-[8px] border border-border-light bg-bg-primary px-[12px] py-[10px]",
    "conn-jump-summary-main": "min-w-0 flex items-center gap-[9px]",
    "conn-jump-summary-icon":
      "inline-flex h-[28px] w-[28px] shrink-0 items-center justify-center rounded-[7px] bg-bg-tertiary",
    "conn-jump-dialog-overlay": {
      "z-index": "40",
      background: "var(--overlay-bg)",
      // WKWebView / old WebKitGTK need the prefixed blur; unprefixed follows.
      "-webkit-backdrop-filter": "blur(4px)",
      "backdrop-filter": "blur(4px)",
    },
    "conn-jump-dialog":
      "grid grid-rows-[auto_minmax(0,1fr)_auto] [inline-size:min(920px,calc(var(--viewport-min-inline)-var(--space-6)))] [block-size:min(680px,calc(var(--viewport-min-block)-var(--space-6)))] overflow-hidden rounded-[14px] border border-border bg-bg-secondary shadow-[0_24px_70px_oklch(0_0_0/0.2),0_6px_18px_oklch(0_0_0/0.12)]",
    "conn-jump-dialog-header":
      "flex items-start gap-[12px] border-b border-border-light px-[20px] pb-[15px] pt-[18px]",
    "conn-jump-dialog-body": "min-h-0 grid grid-cols-[280px_minmax(0,1fr)] overflow-hidden",
    "conn-jump-dialog-footer":
      "flex items-center justify-end gap-[8px] border-t border-border-light px-[20px] pb-[16px] pt-[12px]",
    "conn-jump-chain-pane":
      "min-h-0 min-w-0 border-r border-border-light bg-bg-primary grid grid-cols-[minmax(0,1fr)] grid-rows-[auto_minmax(0,1fr)]",
    "conn-jump-chain-heading":
      "min-h-[44px] flex items-center justify-between gap-[8px] border-b border-border-light px-[12px] text-[0.8214em] font-600 text-text-secondary",
    "conn-jump-chain-list":
      "min-h-0 min-w-0 self-stretch overflow-y-auto p-[10px] grid auto-rows-max content-start gap-[8px]",
    "conn-jump-chain-item":
      "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-[6px] rounded-[8px] border border-border-light bg-bg-secondary p-[6px] transition-[border-color,background-color,box-shadow] duration-150",
    "conn-jump-chain-select":
      "min-w-0 flex items-center gap-[8px] border-0 bg-transparent p-0 text-left",
    "conn-jump-chain-step":
      "inline-flex h-[22px] min-w-[22px] items-center justify-center rounded-[6px] bg-bg-tertiary px-[5px] text-[0.75em] font-700 text-text-tertiary",
    "conn-jump-chain-copy": "min-w-0 grid gap-[1px]",
    "conn-jump-chain-title": "truncate text-[0.8571em] font-600 text-text-primary",
    "conn-jump-chain-subtitle": "truncate text-[0.75em] text-text-tertiary",
    "conn-jump-chain-actions": "grid grid-flow-col auto-cols-[28px] gap-[2px]",
    "conn-jump-detail-pane": "min-h-0 overflow-y-auto p-[16px] grid content-start gap-[12px]",
    "conn-jump-detail-heading": "flex items-start justify-between gap-[10px]",
    "conn-jump-picker-list":
      "grid max-h-[260px] gap-[7px] overflow-y-auto rounded-[8px] border border-border bg-bg-primary p-[6px]",
    "conn-jump-picker-card":
      "min-w-0 grid grid-cols-[32px_minmax(0,1fr)_min-content] items-center gap-[10px] rounded-[7px] border border-transparent bg-transparent px-[8px] py-[8px] text-left transition-[background-color,border-color,box-shadow] duration-150 hover:bg-bg-tertiary focus-visible:bg-bg-tertiary focus-visible:outline-none",
    "conn-jump-picker-icon":
      "inline-flex items-center justify-center h-[32px] w-[32px] shrink-0 rounded-[7px] bg-accent-light text-accent",
    "conn-jump-picker-main": "min-w-0 grid gap-[1px]",
    "conn-jump-picker-name": "truncate text-[0.8571em] font-600 text-text-primary",
    "conn-jump-picker-meta": "truncate text-[0.75em] text-text-tertiary",
    "conn-jump-selected-note":
      "rounded-[7px] border border-border-light bg-bg-primary px-[10px] py-[8px] text-[0.7857em] leading-[1.45] text-text-secondary",
    "conn-jump-editor-empty":
      "min-h-0 min-w-0 self-stretch rounded-[8px] border border-dashed border-border bg-bg-secondary px-[12px] py-[12px] text-center text-[0.8214em] leading-[1.45] text-text-tertiary",
    "conn-jump-detail-empty": "self-center justify-self-center [inline-size:min(320px,100%)]",
    "conn-icon-btn":
      "inline-flex h-[34px] w-[34px] items-center justify-center rounded-[7px] border border-border bg-bg-primary text-text-secondary transition-[background-color,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-[0.55]",
    "conn-host-row":
      "grid ui-fill-inline min-w-0 grid-cols-[minmax(0,1fr)_112px] items-start gap-[10px]",
    "conn-host-col": "min-w-0",
    "conn-port-col": "min-w-0",
    "conn-seg-tabs": "flex gap-[8px]",
    "conn-seg-tab":
      "flex-1 min-h-[38px] flex items-center justify-center gap-[7px] rounded-[8px] border border-border bg-bg-primary px-[12px] py-[7px] text-[0.8571em] font-600 text-text-secondary transition-[background-color,border-color,box-shadow,color] duration-150 hover:border-[color-mix(in_oklch,var(--accent)_36%,var(--border))] hover:text-text-primary focus-visible:border-[color-mix(in_oklch,var(--accent)_36%,var(--border))] focus-visible:text-text-primary focus-visible:outline-none",
    "conn-protocol-grid": "grid grid-cols-[repeat(3,minmax(0,1fr))] gap-[8px]",
    "conn-protocol-card":
      "min-h-[40px] flex items-center justify-center gap-[7px] rounded-[8px] border border-border bg-bg-primary text-[0.8571em] font-600 text-text-secondary transition-[background-color,border-color,box-shadow,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary focus-visible:border-accent focus-visible:shadow-[var(--focus-ring)] focus-visible:outline-none disabled:cursor-default",
    "conn-browse-btn":
      "h-[34px] shrink-0 flex items-center gap-[5px] rounded-[7px] border border-border bg-bg-primary px-[12px] text-[0.8571em] text-text-secondary transition-[background-color,color] duration-150 hover:bg-bg-tertiary hover:text-text-primary",
  },
});
