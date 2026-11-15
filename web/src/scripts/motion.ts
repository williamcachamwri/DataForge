import gsap from 'gsap';
import { ScrollTrigger } from 'gsap/ScrollTrigger';
import Lenis from 'lenis';
import {
  DEFAULT_LOCALE,
  LOCALES,
  translations,
  type Locale,
} from '../i18n/translations';

gsap.registerPlugin(ScrollTrigger);

interface ReleaseCache {
  tag: string;
  url: string;
  ts: number;
}

const THEME_STORAGE_KEY = 'tableminus-theme';
const LOCALE_STORAGE_KEY = 'tableminus-locale';
const RELEASE_CACHE_KEY = 'tableminus-release-cache';
const RELEASE_CACHE_TTL_MS = 10 * 60 * 1000;
const GITHUB_RELEASES_API =
  'https://api.github.com/repos/williamcachamwri/TableMinus/releases/latest';
const GITHUB_TAGS_API =
  'https://api.github.com/repos/williamcachamwri/TableMinus/tags?per_page=1';
const INDICATOR_BASE_W = 120;

let currentLocale: Locale = DEFAULT_LOCALE;
let lenisInstance: Lenis | null = null;
let lenisTickerFn: ((time: number) => void) | null = null;

function isValidLocale(value: string | null): value is Locale {
  return (
    value === 'en' ||
    value === 'vi' ||
    value === 'zh-CN' ||
    value === 'ja' ||
    value === 'es' ||
    value === 'fr'
  );
}

function getDict(locale: Locale): Record<string, string> {
  return translations[locale] ?? translations[DEFAULT_LOCALE];
}

function applyTheme(theme: 'dark' | 'light'): void {
  const root = document.documentElement;
  root.setAttribute('data-theme', theme);
  root.style.colorScheme = theme;

  const metaTheme = document.querySelector('meta[name="theme-color"]');
  if (metaTheme) {
    metaTheme.setAttribute('content', theme === 'light' ? '#FAFAFA' : '#0C0D10');
  }

  const dict = getDict(currentLocale);
  const nextLabel = theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode';

  ['theme-toggle', 'mobile-theme-toggle'].forEach((id) => {
    const btn = document.getElementById(id);
    if (btn) {
      btn.setAttribute('aria-label', nextLabel);
      btn.setAttribute('title', nextLabel);
      btn.setAttribute('aria-pressed', theme === 'light' ? 'true' : 'false');
    }
  });

  const mobileThemeLabel = document.getElementById('mobile-theme-label');
  if (mobileThemeLabel) {
    mobileThemeLabel.textContent =
      theme === 'light'
        ? (dict['nav.themeLight'] ?? 'Light mode')
        : (dict['nav.themeDark'] ?? 'Dark mode');
  }
}

function initThemeToggle(): void {
  const current = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
  applyTheme(current);

  const handleToggle = () => {
    const active = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
    const next: 'dark' | 'light' = active === 'dark' ? 'light' : 'dark';
    applyTheme(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Ignore storage restrictions
    }
  };

  document.getElementById('theme-toggle')?.addEventListener('click', handleToggle);
  document.getElementById('mobile-theme-toggle')?.addEventListener('click', handleToggle);

  const lightQuery = window.matchMedia('(prefers-color-scheme: light)');
  lightQuery.addEventListener('change', (event) => {
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(THEME_STORAGE_KEY);
    } catch {
      saved = null;
    }
    if (saved !== 'light' && saved !== 'dark') {
      applyTheme(event.matches ? 'light' : 'dark');
    }
  });
}

function initLenis(prefersReduced: MediaQueryList): void {
  const destroyLenis = () => {
    if (lenisTickerFn) {
      gsap.ticker.remove(lenisTickerFn);
      lenisTickerFn = null;
    }
    if (lenisInstance) {
      lenisInstance.destroy();
      lenisInstance = null;
    }
  };

  const setupLenis = () => {
    if (prefersReduced.matches || lenisInstance) return;

    const lenis = new Lenis({
      duration: 1.0,
      easing: (t: number) => Math.min(1, 1.001 - Math.pow(2, -10 * t)),
      smoothWheel: true,
      touchMultiplier: 1.5,
    });

    lenis.on('scroll', ScrollTrigger.update);

    const tickerFn = (time: number) => {
      lenis.raf(time * 1000);
    };

    gsap.ticker.add(tickerFn);
    gsap.ticker.lagSmoothing(0);

    lenisInstance = lenis;
    lenisTickerFn = tickerFn;
  };

  setupLenis();

  document.querySelectorAll<HTMLAnchorElement>('a[href^="#"]').forEach((anchor) => {
    anchor.addEventListener('click', (event) => {
      const href = anchor.getAttribute('href');
      if (!href) return;
      if (href === '#') {
        if (lenisInstance) {
          event.preventDefault();
          lenisInstance.scrollTo(0);
        }
        return;
      }
      const target = document.querySelector<HTMLElement>(href);
      if (target && lenisInstance) {
        event.preventDefault();
        lenisInstance.scrollTo(target, { offset: -56 });
      }
    });
  });

  prefersReduced.addEventListener('change', (event) => {
    if (event.matches) {
      destroyLenis();
    } else {
      setupLenis();
    }
  });
}

function initMobileMenu(): void {
  const toggleBtn = document.getElementById('mobile-menu-toggle');
  const panel = document.getElementById('mobile-menu-panel');
  const iconOpen = document.getElementById('mobile-menu-icon-open');
  const iconClose = document.getElementById('mobile-menu-icon-close');
  const navLinks = Array.from(document.querySelectorAll<HTMLElement>('[data-mobile-nav-link]'));

  if (!toggleBtn || !panel) return;

  let isOpen = false;

  const setOpen = (open: boolean, restoreFocus = false) => {
    isOpen = open;
    panel.hidden = !open;
    toggleBtn.setAttribute('aria-expanded', open ? 'true' : 'false');
    if (iconOpen && iconClose) {
      iconOpen.classList.toggle('hidden', open);
      iconClose.classList.toggle('hidden', !open);
    }
    if (!open && restoreFocus) {
      toggleBtn.focus();
    }
  };

  toggleBtn.addEventListener('click', () => {
    setOpen(!isOpen, false);
  });

  navLinks.forEach((link) => {
    link.addEventListener('click', () => {
      setOpen(false, false);
    });
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && isOpen) {
      setOpen(false, true);
    }
  });

  const lgQuery = window.matchMedia('(min-width: 1024px)');
  lgQuery.addEventListener('change', (event) => {
    if (event.matches && isOpen) {
      setOpen(false, false);
    }
  });
}

function applyReleaseToDom(tag: string, url: string): void {
  document.querySelectorAll<HTMLElement>('[data-release-version]').forEach((el) => {
    el.textContent = tag;
  });
  document.querySelectorAll<HTMLAnchorElement>('[data-release-link]').forEach((anchor) => {
    anchor.href = url;
  });
}

function normalizeTag(raw: string): string | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const tag = trimmed.startsWith('v') ? trimmed : `v${trimmed}`;
  return /^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(tag) ? tag : null;
}

async function syncLiveReleaseInfo(): Promise<void> {
  try {
    const rawCache = sessionStorage.getItem(RELEASE_CACHE_KEY);
    if (rawCache) {
      const parsed = JSON.parse(rawCache) as Partial<ReleaseCache>;
      if (
        parsed.tag &&
        parsed.url &&
        typeof parsed.ts === 'number' &&
        Date.now() - parsed.ts < RELEASE_CACHE_TTL_MS
      ) {
        applyReleaseToDom(parsed.tag, parsed.url);
        return;
      }
    }
  } catch {
    // Ignore sessionStorage errors
  }

  const fetchWithTimeout = async <T>(url: string, ms: number): Promise<T | null> => {
    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), ms);
    try {
      const res = await fetch(url, {
        signal: controller.signal,
        headers: { Accept: 'application/vnd.github+json' },
      });
      if (!res.ok) return null;
      return (await res.json()) as T;
    } catch {
      return null;
    } finally {
      window.clearTimeout(timer);
    }
  };

  let nextTag: string | null = null;
  let nextUrl: string | null = null;

  const latest = await fetchWithTimeout<{ tag_name?: string; html_url?: string }>(
    GITHUB_RELEASES_API,
    2500
  );

  if (latest?.tag_name) {
    const tag = normalizeTag(latest.tag_name);
    if (tag) {
      nextTag = tag;
      nextUrl =
        latest.html_url ||
        `https://github.com/williamcachamwri/TableMinus/releases/tag/${encodeURIComponent(tag)}`;
    }
  }

  if (!nextTag) {
    const tags = await fetchWithTimeout<Array<{ name?: string }>>(GITHUB_TAGS_API, 2200);
    if (Array.isArray(tags) && tags[0]?.name) {
      const tag = normalizeTag(tags[0].name);
      if (tag) {
        nextTag = tag;
        nextUrl = `https://github.com/williamcachamwri/TableMinus/releases/tag/${encodeURIComponent(tag)}`;
      }
    }
  }

  if (nextTag && nextUrl) {
    applyReleaseToDom(nextTag, nextUrl);
    try {
      const cache: ReleaseCache = { tag: nextTag, url: nextUrl, ts: Date.now() };
      sessionStorage.setItem(RELEASE_CACHE_KEY, JSON.stringify(cache));
    } catch {
      // Ignore sessionStorage errors
    }
  }
}

function initDriverFilter(): void {
  const filterBtns = Array.from(document.querySelectorAll<HTMLButtonElement>('[data-driver-filter]'));
  const cards = Array.from(document.querySelectorAll<HTMLElement>('[data-driver-card]'));
  const filler = document.getElementById('driver-tablet-filler');
  if (filterBtns.length === 0 || cards.length === 0) return;

  filterBtns.forEach((btn) => {
    btn.addEventListener('click', () => {
      const filter = btn.dataset.driverFilter ?? 'all';
      let visibleCount = 0;

      filterBtns.forEach((b) => {
        b.setAttribute('aria-pressed', b === btn ? 'true' : 'false');
      });

      cards.forEach((card) => {
        const category = card.dataset.driverCategory ?? '';
        const visible = filter === 'all' || category === filter;
        card.hidden = !visible;
        if (visible) visibleCount += 1;
      });

      if (filler) {
        filler.hidden = visibleCount % 2 === 0;
      }

      ScrollTrigger.refresh();
    });
  });
}

function initInstallSwitcherAndCopy(): void {
  const installTabs = Array.from(document.querySelectorAll<HTMLButtonElement>('[data-install-tab]'));
  const codeEl = document.getElementById('cli-install-code');
  const copyBtn = document.getElementById('copy-cli-btn') as HTMLButtonElement | null;
  const copyLabel = copyBtn?.querySelector<HTMLElement>('[data-copy-label]');

  installTabs.forEach((btn) => {
    btn.addEventListener('click', () => {
      const cmd = btn.dataset.installCmd;
      if (!cmd) return;
      installTabs.forEach((b) => b.setAttribute('aria-selected', b === btn ? 'true' : 'false'));
      if (codeEl) codeEl.textContent = cmd;
      if (copyBtn) {
        copyBtn.dataset.copyText = cmd;
        copyBtn.dataset.copied = 'false';
      }
      if (copyLabel) {
        copyLabel.textContent = getDict(currentLocale)['cli.copy'] ?? 'Copy';
      }
    });
  });

  if (copyBtn && copyLabel) {
    let resetTimer: number | undefined;
    copyBtn.addEventListener('click', async () => {
      const text = copyBtn.dataset.copyText ?? '';
      const dict = getDict(currentLocale);
      const copyWord = dict['cli.copy'] ?? 'Copy';
      const copiedWord = dict['cli.copied'] ?? 'Copied';
      try {
        await navigator.clipboard.writeText(text);
        copyBtn.dataset.copied = 'true';
        copyLabel.textContent = copiedWord;
        window.clearTimeout(resetTimer);
        resetTimer = window.setTimeout(() => {
          copyBtn.dataset.copied = 'false';
          copyLabel.textContent = getDict(currentLocale)['cli.copy'] ?? copyWord;
        }, 1600);
      } catch {
        copyLabel.textContent = 'Select manually';
        window.clearTimeout(resetTimer);
        resetTimer = window.setTimeout(() => {
          copyLabel.textContent = getDict(currentLocale)['cli.copy'] ?? copyWord;
        }, 1800);
      }
    });
  }
}

export function initTableMinusMotion(): void {
  const prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)');

  initThemeToggle();
  initLenis(prefersReduced);
  initMobileMenu();
  initDriverFilter();
  initInstallSwitcherAndCopy();
  void syncLiveReleaseInfo();

  // Scroll progress hairline
  const mm = gsap.matchMedia();
  mm.add(
    {
      reduce: '(prefers-reduced-motion: reduce)',
      animate: '(prefers-reduced-motion: no-preference)',
    },
    (context) => {
      const { reduce } = context.conditions as { reduce: boolean; animate: boolean };
      const progressEl = document.getElementById('df-scroll-progress');
      if (!progressEl) return;

      if (reduce) {
        ScrollTrigger.create({
          start: 0,
          end: 'max',
          onUpdate: (self) => {
            gsap.set(progressEl, { scaleX: self.progress });
          },
        });
        return;
      }

      gsap.fromTo(
        progressEl,
        { scaleX: 0 },
        {
          scaleX: 1,
          ease: 'none',
          scrollTrigger: {
            start: 0,
            end: 'max',
            scrub: true,
          },
        }
      );
    }
  );

  // Workbench 6-Tab Switcher
  const tablist = document.querySelector<HTMLElement>('[data-tablist]');
  const indicatorEl = document.getElementById('workbench-tab-indicator');
  const tabButtons = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role="tab"][data-tab-target]')
  );
  const panels = Array.from(
    document.querySelectorAll<HTMLElement>('[role="tabpanel"][data-panel-id]')
  );

  let activeId =
    tabButtons.find((btn) => btn.getAttribute('aria-selected') === 'true')?.dataset.tabTarget ??
    'sql-grid';

  function positionIndicator(targetBtn: HTMLButtonElement, immediate: boolean): void {
    if (!indicatorEl || !tablist) return;
    const x = targetBtn.offsetLeft;
    const scaleX = targetBtn.offsetWidth / INDICATOR_BASE_W;
    const height = targetBtn.offsetHeight;

    indicatorEl.style.height = `${height}px`;

    if (immediate || prefersReduced.matches) {
      gsap.set(indicatorEl, { x, y: 0, scaleX, opacity: 1 });
    } else {
      gsap.to(indicatorEl, {
        x,
        y: 0,
        scaleX,
        opacity: 1,
        duration: 0.18,
        ease: 'power3.out',
        overwrite: 'auto',
      });
    }
    tablist.dataset.indicatorReady = 'true';
  }

  const initialBtn = tabButtons.find((btn) => btn.dataset.tabTarget === activeId);
  if (initialBtn) {
    positionIndicator(initialBtn, true);
  }

  if (document.fonts?.ready) {
    document.fonts.ready.then(() => {
      const currentBtn = tabButtons.find((btn) => btn.dataset.tabTarget === activeId);
      if (currentBtn) positionIndicator(currentBtn, true);
    });
  }

  if (tablist && typeof ResizeObserver !== 'undefined') {
    const ro = new ResizeObserver(() => {
      const currentBtn = tabButtons.find((btn) => btn.dataset.tabTarget === activeId);
      if (currentBtn) positionIndicator(currentBtn, true);
    });
    ro.observe(tablist);
  }

  function activateTab(nextId: string, moveFocus = false): void {
    if (nextId === activeId) {
      if (moveFocus) {
        tabButtons.find((b) => b.dataset.tabTarget === nextId)?.focus();
      }
      return;
    }

    const nextPanel = panels.find((p) => p.dataset.panelId === nextId);
    if (!nextPanel) return;

    activeId = nextId;

    let nextBtn: HTMLButtonElement | undefined;
    tabButtons.forEach((btn) => {
      const isSelected = btn.dataset.tabTarget === nextId;
      btn.setAttribute('aria-selected', isSelected ? 'true' : 'false');
      btn.tabIndex = isSelected ? 0 : -1;
      if (isSelected) {
        nextBtn = btn;
        if (moveFocus) btn.focus();
      }
    });

    if (nextBtn) {
      positionIndicator(nextBtn, false);
    }

    panels.forEach((panel) => {
      panel.hidden = panel.dataset.panelId !== nextId;
    });

    if (!prefersReduced.matches) {
      gsap.fromTo(
        nextPanel,
        { y: 4 },
        { y: 0, duration: 0.16, ease: 'power3.out', clearProps: 'transform' }
      );
    }

    ScrollTrigger.refresh();
  }

  tabButtons.forEach((btn, idx) => {
    btn.addEventListener('click', () => {
      const target = btn.dataset.tabTarget;
      if (target) activateTab(target, false);
    });

    btn.addEventListener('keydown', (event) => {
      let nextIdx = idx;
      if (event.key === 'ArrowRight') {
        nextIdx = (idx + 1) % tabButtons.length;
      } else if (event.key === 'ArrowLeft') {
        nextIdx = (idx - 1 + tabButtons.length) % tabButtons.length;
      } else if (event.key === 'Home') {
        nextIdx = 0;
      } else if (event.key === 'End') {
        nextIdx = tabButtons.length - 1;
      } else {
        return;
      }

      event.preventDefault();
      const target = tabButtons[nextIdx]?.dataset.tabTarget;
      if (target) activateTab(target, true);
    });
  });

  // 6-Locale Instant Client-Side i18n Switcher
  const langMenu = document.querySelector<HTMLElement>('[data-lang-menu]');
  const langToggle = document.getElementById('lang-toggle');
  const langDropdown = document.getElementById('lang-dropdown');
  const langShortEl = document.getElementById('lang-current-short');
  const langOptions = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[data-locale-option]')
  );
  const copyBtn = document.getElementById('copy-cli-btn') as HTMLButtonElement | null;
  const copyLabel = copyBtn?.querySelector<HTMLElement>('[data-copy-label]');

  function setMenuOpen(open: boolean): void {
    if (!langMenu || !langToggle || !langDropdown) return;
    langMenu.dataset.open = open ? 'true' : 'false';
    langToggle.setAttribute('aria-expanded', open ? 'true' : 'false');
    langDropdown.hidden = !open;
  }

  function applyLocale(locale: Locale, persist = true): void {
    currentLocale = locale;
    const dict = getDict(locale);
    document.documentElement.setAttribute('lang', locale);

    if (dict['meta.title']) {
      document.title = dict['meta.title'];
    }

    document.querySelectorAll<HTMLElement>('[data-i18n]').forEach((el) => {
      const key = el.dataset.i18n;
      if (!key) return;
      const value = dict[key];
      if (typeof value === 'string') {
        el.textContent = value;
      }
    });

    document.querySelectorAll<HTMLElement>('[data-i18n-aria-label]').forEach((el) => {
      const key = el.dataset.i18nAriaLabel;
      if (!key) return;
      const value = dict[key];
      if (typeof value === 'string') {
        el.setAttribute('aria-label', value);
      }
    });

    if (copyLabel && copyBtn?.dataset.copied !== 'true' && dict['cli.copy']) {
      copyLabel.textContent = dict['cli.copy'];
    }

    const activeTheme = document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark';
    applyTheme(activeTheme);

    const localeMeta = LOCALES.find((item) => item.code === locale);
    if (langShortEl && localeMeta) {
      langShortEl.textContent = localeMeta.short;
    }
    if (langToggle && dict['nav.languageLabel']) {
      langToggle.setAttribute('aria-label', dict['nav.languageLabel']);
    }

    langOptions.forEach((opt) => {
      opt.setAttribute('aria-selected', opt.dataset.localeOption === locale ? 'true' : 'false');
    });

    window.requestAnimationFrame(() => {
      const currentBtn = tabButtons.find((btn) => btn.dataset.tabTarget === activeId);
      if (currentBtn) positionIndicator(currentBtn, true);
      ScrollTrigger.refresh();
    });

    if (persist) {
      try {
        localStorage.setItem(LOCALE_STORAGE_KEY, locale);
      } catch {
        // Ignore storage restrictions
      }
    }
  }

  let initialLocale: Locale = DEFAULT_LOCALE;
  try {
    const saved = localStorage.getItem(LOCALE_STORAGE_KEY);
    if (isValidLocale(saved)) {
      initialLocale = saved;
    }
  } catch {
    initialLocale = DEFAULT_LOCALE;
  }

  if (initialLocale !== DEFAULT_LOCALE) {
    applyLocale(initialLocale, false);
  } else {
    langOptions.forEach((opt) => {
      opt.setAttribute('aria-selected', opt.dataset.localeOption === DEFAULT_LOCALE ? 'true' : 'false');
    });
  }

  langOptions.forEach((opt) => {
    opt.addEventListener('click', () => {
      const nextLocale = opt.dataset.localeOption ?? null;
      if (isValidLocale(nextLocale)) {
        applyLocale(nextLocale, true);
      }
      setMenuOpen(false);
      if (langMenu?.contains(opt)) {
        langToggle?.focus();
      }
    });
  });

  if (langToggle && langDropdown && langMenu) {
    langToggle.addEventListener('click', (event) => {
      event.stopPropagation();
      const isOpen = langMenu.dataset.open === 'true';
      setMenuOpen(!isOpen);
    });

    document.addEventListener('click', (event) => {
      if (!langMenu.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    });

    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && langMenu.dataset.open === 'true') {
        setMenuOpen(false);
        langToggle.focus();
      }
    });
  }
}
