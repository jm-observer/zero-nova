/**
 * i18n - Internationalization core module
 * Lightweight translation system for OpenFlux client
 */

export type Locale = 'zh' | 'en';

// Current locale
let currentLocale: Locale = 'zh';

// Language packs registry
const messages: Record<Locale, Record<string, string>> = {
    zh: {},
    en: {},
};

/**
 * Translate a key to the current locale.
 * Supports {0} {1} positional placeholders.
 */
export function t(key: string, ...args: (string | number)[]): string {
    const template = messages[currentLocale]?.[key] || messages['en']?.[key] || key;
    if (args.length === 0) return template;
    return template.replace(/\{(\d+)\}/g, (_, i) => String(args[+i] ?? ''));
}

/**
 * Set locale and refresh DOM
 */
export function setLocale(locale: Locale): void {
    currentLocale = locale;
    localStorage.setItem('openflux-locale', locale);
    applyI18nToDOM();
    document.dispatchEvent(new CustomEvent('locale-changed', { detail: locale }));
}

/**
 * Get current locale
 */
export function getLocale(): Locale {
    return currentLocale;
}

/**
 * Initialize i18n: detect user preference or browser language
 */
export function initI18n(zhPack: Record<string, string>, enPack: Record<string, string>): void {
    messages.zh = zhPack;
    messages.en = enPack;

    const saved = localStorage.getItem('openflux-locale') as Locale | null;
    if (saved && (saved === 'zh' || saved === 'en')) {
        currentLocale = saved;
    } else {
        currentLocale = navigator.language.startsWith('zh') ? 'zh' : 'en';
    }
}

/**
 * Batch apply translations to DOM elements with data-i18n attributes.
 * Call after locale change or initial load.
 *
 *   data-i18n="key"             → textContent
 *   data-i18n-placeholder="key" → placeholder
 *   data-i18n-title="key"       → title attribute
 *   data-i18n-html="key"        → innerHTML (use sparingly)
 */
export function applyI18nToDOM(): void {
    document.querySelectorAll('[data-i18n]').forEach(el => {
        const key = el.getAttribute('data-i18n')!;
        el.textContent = t(key);
    });
    document.querySelectorAll('[data-i18n-placeholder]').forEach(el => {
        const key = el.getAttribute('data-i18n-placeholder')!;
        (el as HTMLInputElement).placeholder = t(key);
    });
    document.querySelectorAll('[data-i18n-title]').forEach(el => {
        const key = el.getAttribute('data-i18n-title')!;
        el.setAttribute('title', t(key));
    });
    document.querySelectorAll('[data-i18n-html]').forEach(el => {
        const key = el.getAttribute('data-i18n-html')!;
        el.innerHTML = t(key);
    });
}
