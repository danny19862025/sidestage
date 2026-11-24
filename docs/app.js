/* =========================================================
   Sidestage Modern Reveal Slide Deck — Auto-Slide & Controls
   ========================================================= */

(() => {
  'use strict';

  /* 1. Theme Toggle */
  const html = document.documentElement;
  const themeToggle = document.getElementById('themeToggle');
  const savedTheme = localStorage.getItem('sidestage-web-theme');
  if (savedTheme === 'light' || savedTheme === 'dark') {
    html.setAttribute('data-theme', savedTheme);
  }
  themeToggle?.addEventListener('click', () => {
    const next = html.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
    html.setAttribute('data-theme', next);
    localStorage.setItem('sidestage-web-theme', next);
  });

  /* 2. Reveal Slide Deck + Auto-Slide Engine */
  const slides = Array.from(document.querySelectorAll('.slide'));
  const totalSlides = slides.length;
  const hudDots = document.getElementById('hudDots');
  const topbarSlideLabel = document.getElementById('topbarSlideLabel');
  const autoSlideFill = document.getElementById('autoSlideFill');
  const autoSlideToggle = document.getElementById('autoSlideToggle');
  const autoSlideStatus = document.getElementById('autoSlideStatus');
  const speedBtn = document.getElementById('speedBtn');
  const prevSlideBtn = document.getElementById('prevSlideBtn');
  const nextSlideBtn = document.getElementById('nextSlideBtn');
  const brandHomeBtn = document.getElementById('brandHomeBtn');
  const orb1 = document.querySelector('.orb-1');
  const orb2 = document.querySelector('.orb-2');

  const speeds = [5000, 7000, 10000];
  let speedIndex = 1; // default 7s
  let currentIndex = 0;
  let isPlaying = true;
  let elapsed = 0;
  let lastTick = performance.now();
  let modalOpen = false;

  // Build pagination dots
  const dotButtons = [];
  if (hudDots) {
    slides.forEach((slide, idx) => {
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.className = `hud-dot${idx === 0 ? ' active' : ''}`;
      const title = slide.getAttribute('data-title') || `Slide ${idx + 1}`;
      btn.title = `${idx + 1}. ${title}`;
      btn.setAttribute('aria-label', `Go to slide ${idx + 1}: ${title}`);
      btn.addEventListener('click', () => goToSlide(idx));
      hudDots.appendChild(btn);
      dotButtons.push(btn);
    });
  }

  const updateSlideClasses = () => {
    slides.forEach((slide, idx) => {
      slide.classList.remove('active', 'prev', 'next');
      if (idx === currentIndex) {
        slide.classList.add('active');
      } else if (idx < currentIndex) {
        slide.classList.add('prev');
      } else {
        slide.classList.add('next');
      }
    });

    dotButtons.forEach((dot, idx) => {
      dot.classList.toggle('active', idx === currentIndex);
    });

    const title = slides[currentIndex]?.getAttribute('data-title') || '';
    const num = String(currentIndex + 1).padStart(2, '0');
    const tot = String(totalSlides).padStart(2, '0');
    if (topbarSlideLabel) {
      topbarSlideLabel.textContent = `${num} / ${tot} · ${title}`;
    }

    // Subtle ambient orb shift per slide
    const shift = (currentIndex / Math.max(1, totalSlides - 1)) * 90;
    if (orb1) orb1.style.transform = `translate(${shift}px, ${shift * 0.45}px)`;
    if (orb2) orb2.style.transform = `translate(${-shift}px, ${-shift * 0.45}px)`;

    updateOverviewHighlight();
  };

  const resetTimer = () => {
    elapsed = 0;
    lastTick = performance.now();
    if (autoSlideFill) autoSlideFill.style.width = '0%';
  };

  const goToSlide = (idx) => {
    const wrapped = ((idx % totalSlides) + totalSlides) % totalSlides;
    currentIndex = wrapped;
    updateSlideClasses();
    resetTimer();
  };

  const nextSlide = () => goToSlide(currentIndex + 1);
  const prevSlide = () => goToSlide(currentIndex - 1);

  const setPlaying = (playing) => {
    isPlaying = playing;
    lastTick = performance.now();
    autoSlideToggle?.classList.toggle('playing', isPlaying);
    autoSlideToggle?.setAttribute('aria-label', isPlaying ? 'Pause auto-slide' : 'Resume auto-slide');
    if (autoSlideStatus) autoSlideStatus.textContent = isPlaying ? 'Auto' : 'Paused';
    if (!isPlaying && autoSlideFill) {
      // Keep current visual progress frozen when paused
    }
  };

  // Auto-slide animation loop
  const tick = (now) => {
    const dt = now - lastTick;
    lastTick = now;

    if (isPlaying && !modalOpen && !document.hidden) {
      const duration = speeds[speedIndex];
      elapsed += dt;
      const pct = Math.min(100, (elapsed / duration) * 100);
      if (autoSlideFill) autoSlideFill.style.width = `${pct}%`;

      if (elapsed >= duration) {
        nextSlide();
      }
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  // Controls bindings
  prevSlideBtn?.addEventListener('click', prevSlide);
  nextSlideBtn?.addEventListener('click', nextSlide);
  brandHomeBtn?.addEventListener('click', () => goToSlide(0));
  autoSlideToggle?.addEventListener('click', () => setPlaying(!isPlaying));

  speedBtn?.addEventListener('click', () => {
    speedIndex = (speedIndex + 1) % speeds.length;
    speedBtn.textContent = `${speeds[speedIndex] / 1000}s`;
    resetTimer();
  });

  document.querySelectorAll('[data-go-slide]').forEach((btn) => {
    btn.addEventListener('click', () => {
      const target = Number(btn.getAttribute('data-go-slide'));
      if (!Number.isNaN(target)) goToSlide(target);
    });
  });

  // Fullscreen toggle
  const toggleFullscreen = () => {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen?.().catch(() => {});
    } else {
      document.exitFullscreen?.().catch(() => {});
    }
  };
  document.getElementById('fullscreenBtn')?.addEventListener('click', toggleFullscreen);

  /* 3. Overview Grid Modal */
  const overviewModal = document.getElementById('overviewModal');
  const overviewGrid = document.getElementById('overviewGrid');
  const overviewToggleBtn = document.getElementById('overviewToggleBtn');
  const overviewCloseBtn = document.getElementById('overviewCloseBtn');
  const overviewTiles = [];

  if (overviewGrid) {
    slides.forEach((slide, idx) => {
      const tile = document.createElement('button');
      tile.type = 'button';
      tile.className = 'overview-tile';
      const num = String(idx + 1).padStart(2, '0');
      const title = slide.getAttribute('data-title') || `Slide ${idx + 1}`;
      tile.innerHTML = `<span class="overview-num">SLIDE ${num}</span><span class="overview-title">${title}</span>`;
      tile.addEventListener('click', () => {
        goToSlide(idx);
        closeOverview();
      });
      overviewGrid.appendChild(tile);
      overviewTiles.push(tile);
    });
  }

  function updateOverviewHighlight() {
    overviewTiles.forEach((tile, idx) => {
      tile.classList.toggle('current', idx === currentIndex);
    });
  }

  const openOverview = () => {
    if (!overviewModal) return;
    overviewModal.hidden = false;
    modalOpen = true;
    updateOverviewHighlight();
  };

  const closeOverview = () => {
    if (!overviewModal) return;
    overviewModal.hidden = true;
    modalOpen = !lightbox?.hidden;
    lastTick = performance.now();
  };

  overviewToggleBtn?.addEventListener('click', () => {
    if (overviewModal?.hidden) openOverview();
    else closeOverview();
  });
  overviewCloseBtn?.addEventListener('click', closeOverview);

  /* 4. Keyboard, Wheel & Touch Swipe Navigation */
  document.addEventListener('keydown', (e) => {
    const tag = document.activeElement?.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA') return;

    if (e.key === 'Escape') {
      if (lightbox && !lightbox.hidden) closeLightbox();
      else if (overviewModal && !overviewModal.hidden) closeOverview();
      return;
    }

    if (modalOpen) {
      if (e.key.toLowerCase() === 'o') closeOverview();
      return;
    }

    if (e.key === 'ArrowRight' || e.key === 'PageDown') {
      e.preventDefault();
      nextSlide();
    } else if (e.key === 'ArrowLeft' || e.key === 'PageUp') {
      e.preventDefault();
      prevSlide();
    } else if (e.key === ' ') {
      e.preventDefault();
      setPlaying(!isPlaying);
    } else if (e.key.toLowerCase() === 'o') {
      openOverview();
    } else if (e.key.toLowerCase() === 'f') {
      toggleFullscreen();
    }
  });

  // Wheel navigation (debounced)
  let wheelLock = 0;
  window.addEventListener(
    'wheel',
    (e) => {
      if (modalOpen) return;
      if (e.target.closest('.sim-scroll, pre')) return;
      const now = Date.now();
      if (now - wheelLock < 650) return;
      if (Math.abs(e.deltaY) > 35 || Math.abs(e.deltaX) > 35) {
        wheelLock = now;
        if (e.deltaY > 0 || e.deltaX > 0) nextSlide();
        else prevSlide();
      }
    },
    { passive: true }
  );

  // Touch swipe
  let touchStartX = 0;
  window.addEventListener(
    'touchstart',
    (e) => {
      touchStartX = e.changedTouches[0].screenX;
    },
    { passive: true }
  );
  window.addEventListener(
    'touchend',
    (e) => {
      if (modalOpen) return;
      const diff = e.changedTouches[0].screenX - touchStartX;
      if (Math.abs(diff) > 55) {
        if (diff < 0) nextSlide();
        else prevSlide();
      }
    },
    { passive: true }
  );

  /* 5. Interactive Sidestage Desktop Simulator (Slide 2) */
  const desktopFrame = document.getElementById('desktopFrame');
  const pauseOnDemoInteract = () => {
    if (isPlaying) setPlaying(false);
  };
  desktopFrame?.addEventListener('pointerdown', pauseOnDemoInteract);

  const sectionsData = [
    {
      id: 'coding',
      name: 'Coding',
      collapsed: false,
      apps: [
        { name: 'VS Code', short: 'VS', color: '#007acc' },
        { name: 'Terminal', short: '>_', color: '#2b2f3a' },
        { name: 'Git', short: 'Gt', color: '#f05032' },
      ],
    },
    {
      id: 'work',
      name: 'Work',
      collapsed: false,
      apps: [
        { name: 'Browser', short: 'Br', color: '#2563eb' },
        { name: 'Files', short: 'Fl', color: '#eab308' },
        { name: 'Notes', short: 'Nt', color: '#8b5cf6' },
      ],
    },
    {
      id: 'games',
      name: 'Games',
      collapsed: true,
      apps: [
        { name: 'Steam', short: 'St', color: '#1b2838' },
        { name: 'Xbox', short: 'Xb', color: '#107c10' },
      ],
    },
  ];

  let activeWin = {
    id: 'win-notes',
    title: 'Design notes',
    app: 'Writer',
    tag: 'Workspace',
    desc: 'Maximized windows respect Sidestage\'s reserved edge space. Click − above to send this window to the Minimized tray, or click any card to restore it.',
    gradient: 'linear-gradient(135deg, #ee776b 0%, #c667b7 52%, #6d5ff5 100%)',
    dot: '#f97316',
  };

  let minimizedWindows = [
    {
      id: 'win-report',
      title: 'Quarterly report.odt',
      app: 'Writer',
      tag: 'Document',
      desc: 'Restored cleanly from your minimized tray in one click — without rearranging or hiding any other open windows.',
      gradient: 'linear-gradient(135deg, #3b82f6 0%, #6366f1 100%)',
      dot: '#38bdf8',
    },
    {
      id: 'win-charmap',
      title: 'Character Map',
      app: 'Utilities',
      tag: 'Live Preview',
      desc: 'On Windows 11, minimized windows show live DWM thumbnails. On KDE Plasma & GNOME, Sidestage captures crisp minimize-time snapshots.',
      gradient: 'linear-gradient(135deg, #10b981 0%, #0ea5e9 100%)',
      dot: '#34d399',
    },
    {
      id: 'win-release',
      title: 'v0.3.0 Release Notes',
      app: 'Browser',
      tag: 'Web',
      desc: 'Multiple minimized windows from the same app group into clean stacks, with Restore All one right-click away.',
      gradient: 'linear-gradient(135deg, #f59e0b 0%, #ec4899 100%)',
      dot: '#fbbf24',
    },
  ];

  let searchQuery = '';
  const simSectionsEl = document.getElementById('simSections');
  const simMinListEl = document.getElementById('simMinList');
  const simMinCountEl = document.getElementById('simMinCount');
  const simActiveTitle = document.getElementById('simActiveTitle');
  const simActiveHeading = document.getElementById('simActiveHeading');
  const simActiveDesc = document.getElementById('simActiveDesc');
  const simActiveTag = document.getElementById('simActiveTag');
  const simActiveBody = document.getElementById('simActiveBody');
  const simActiveDot = document.getElementById('simActiveDot');
  const simToast = document.getElementById('simToast');

  let toastTimer = null;
  const showToast = (msg) => {
    if (!simToast) return;
    simToast.textContent = msg;
    simToast.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => simToast.classList.remove('show'), 2000);
  };

  const renderActiveWindow = () => {
    if (!activeWin) return;
    if (simActiveTitle) simActiveTitle.textContent = `${activeWin.title} — ${activeWin.app}`;
    if (simActiveHeading) simActiveHeading.textContent = activeWin.title;
    if (simActiveDesc) simActiveDesc.textContent = activeWin.desc;
    if (simActiveTag) simActiveTag.textContent = activeWin.tag;
    if (simActiveBody) simActiveBody.style.background = activeWin.gradient;
    if (simActiveDot) simActiveDot.style.background = activeWin.dot;
  };

  const renderSimulator = () => {
    const q = searchQuery.trim().toLowerCase();
    if (simSectionsEl) {
      simSectionsEl.innerHTML = '';
      sectionsData.forEach((sec) => {
        const filteredApps = q
          ? sec.apps.filter((a) => a.name.toLowerCase().includes(q) || sec.name.toLowerCase().includes(q))
          : sec.apps;
        if (q && filteredApps.length === 0) return;

        const secEl = document.createElement('div');
        secEl.className = `sim-section${sec.collapsed && !q ? ' collapsed' : ''}`;

        const btn = document.createElement('button');
        btn.className = 'sim-section-btn';
        btn.type = 'button';
        btn.innerHTML = `<span class="sim-chevron">▾</span><span>${sec.name}</span><span class="sim-section-count">${filteredApps.length}</span>`;
        btn.addEventListener('click', () => {
          sec.collapsed = !sec.collapsed;
          renderSimulator();
        });

        const grid = document.createElement('div');
        grid.className = 'sim-apps-grid';
        filteredApps.forEach((app) => {
          const appBtn = document.createElement('button');
          appBtn.className = 'sim-app-btn';
          appBtn.type = 'button';
          appBtn.innerHTML = `<span class="sim-app-icon" style="background:${app.color}">${app.short}</span><span class="sim-app-name">${app.name}</span>`;
          appBtn.addEventListener('click', () => showToast(`Launched ${app.name}`));
          grid.appendChild(appBtn);
        });

        secEl.appendChild(btn);
        secEl.appendChild(grid);
        simSectionsEl.appendChild(secEl);
      });
    }

    const filteredMin = q
      ? minimizedWindows.filter((w) => w.title.toLowerCase().includes(q) || w.app.toLowerCase().includes(q))
      : minimizedWindows;
    if (simMinCountEl) simMinCountEl.textContent = String(filteredMin.length);

    if (simMinListEl) {
      simMinListEl.innerHTML = '';
      if (filteredMin.length === 0) {
        const empty = document.createElement('div');
        empty.className = 'sim-min-empty';
        empty.textContent = q ? 'No matching minimized windows' : 'Minimize a window to see it here';
        simMinListEl.appendChild(empty);
      } else {
        filteredMin.forEach((win) => {
          const card = document.createElement('button');
          card.className = 'sim-min-card';
          card.type = 'button';
          card.innerHTML = `<div class="sim-min-thumb" style="background:${win.gradient}"><span>${win.title}</span></div><div class="sim-min-title"><span>${win.title}</span><span>↗</span></div>`;
          card.addEventListener('click', () => {
            const prev = activeWin;
            minimizedWindows = minimizedWindows.filter((item) => item.id !== win.id);
            if (prev) minimizedWindows.unshift(prev);
            activeWin = win;
            renderActiveWindow();
            renderSimulator();
            showToast(`Restored "${win.title}"`);
          });
          simMinListEl.appendChild(card);
        });
      }
    }
  };

  const minimizeActive = () => {
    if (!activeWin) return;
    const prev = activeWin;
    minimizedWindows.unshift(prev);
    if (minimizedWindows.length > 1) activeWin = minimizedWindows.pop();
    renderActiveWindow();
    renderSimulator();
    showToast(`Minimized "${prev.title}"`);
  };

  document.getElementById('simMinimizeBtn')?.addEventListener('click', minimizeActive);
  document.getElementById('simMinimizeActionBtn')?.addEventListener('click', minimizeActive);
  document.getElementById('simRestoreAllBtn')?.addEventListener('click', () => {
    if (minimizedWindows.length === 0) return showToast('All windows are open');
    activeWin = minimizedWindows[0];
    minimizedWindows = [];
    renderActiveWindow();
    renderSimulator();
    showToast('Restored all minimized windows');
  });

  const simSearchWrap = document.getElementById('simSearchWrap');
  const simSearchInput = document.getElementById('simSearchInput');
  document.getElementById('simSearchToggle')?.addEventListener('click', () => {
    if (!simSearchWrap) return;
    simSearchWrap.hidden = !simSearchWrap.hidden;
    if (!simSearchWrap.hidden) simSearchInput?.focus();
    else {
      searchQuery = '';
      if (simSearchInput) simSearchInput.value = '';
      renderSimulator();
    }
  });
  simSearchInput?.addEventListener('input', (e) => {
    searchQuery = e.target.value;
    renderSimulator();
  });
  document.getElementById('simSearchClear')?.addEventListener('click', () => {
    searchQuery = '';
    if (simSearchInput) simSearchInput.value = '';
    renderSimulator();
  });

  const demoDarkBtn = document.getElementById('demoDarkBtn');
  const demoLightBtn = document.getElementById('demoLightBtn');
  const demoEdgeBtn = document.getElementById('demoEdgeBtn');
  const demoAutohideBtn = document.getElementById('demoAutohideBtn');
  const edgeHotspot = document.getElementById('edgeHotspot');
  const simSidebar = document.getElementById('simSidebar');

  demoDarkBtn?.addEventListener('click', () => {
    pauseOnDemoInteract();
    desktopFrame?.setAttribute('data-sidebar-theme', 'dark');
    demoDarkBtn.classList.add('active');
    demoLightBtn?.classList.remove('active');
  });
  demoLightBtn?.addEventListener('click', () => {
    pauseOnDemoInteract();
    desktopFrame?.setAttribute('data-sidebar-theme', 'light');
    demoLightBtn.classList.add('active');
    demoDarkBtn?.classList.remove('active');
  });
  demoEdgeBtn?.addEventListener('click', () => {
    pauseOnDemoInteract();
    const next = desktopFrame?.getAttribute('data-edge') === 'left' ? 'right' : 'left';
    desktopFrame?.setAttribute('data-edge', next);
    demoEdgeBtn.textContent = `Edge: ${next === 'left' ? 'Left' : 'Right'}`;
  });
  demoAutohideBtn?.addEventListener('click', () => {
    pauseOnDemoInteract();
    const next = desktopFrame?.getAttribute('data-autohide') !== 'true';
    desktopFrame?.setAttribute('data-autohide', String(next));
    desktopFrame?.classList.remove('autohide-revealed');
    demoAutohideBtn.classList.toggle('active', next);
    demoAutohideBtn.textContent = `Auto-hide: ${next ? 'On' : 'Off'}`;
  });

  edgeHotspot?.addEventListener('mouseenter', () => {
    if (desktopFrame?.getAttribute('data-autohide') === 'true') desktopFrame.classList.add('autohide-revealed');
  });
  simSidebar?.addEventListener('mouseenter', () => {
    if (desktopFrame?.getAttribute('data-autohide') === 'true') desktopFrame.classList.add('autohide-revealed');
  });
  simSidebar?.addEventListener('mouseleave', () => {
    if (desktopFrame?.getAttribute('data-autohide') === 'true') desktopFrame.classList.remove('autohide-revealed');
  });

  renderActiveWindow();
  renderSimulator();

  /* 6. Screenshot Lightbox */
  const lightbox = document.getElementById('lightbox');
  const lightboxImg = document.getElementById('lightboxImg');
  const lightboxCaption = document.getElementById('lightboxCaption');

  const closeLightbox = () => {
    if (!lightbox) return;
    lightbox.hidden = true;
    modalOpen = !overviewModal?.hidden;
    lastTick = performance.now();
  };

  document.querySelectorAll('[data-lightbox-src]').forEach((card) => {
    card.addEventListener('click', () => {
      const src = card.getAttribute('data-lightbox-src');
      const caption = card.getAttribute('data-lightbox-caption') || '';
      if (lightboxImg && src) lightboxImg.src = src;
      if (lightboxCaption) lightboxCaption.textContent = caption;
      if (lightbox) {
        lightbox.hidden = false;
        modalOpen = true;
      }
    });
  });

  document.getElementById('lightboxClose')?.addEventListener('click', closeLightbox);
  document.getElementById('lightboxBackdrop')?.addEventListener('click', closeLightbox);

  /* 7. OS & Distro Switcher (Slide 8) */
  const osTabs = document.querySelectorAll('.os-tab[data-os]');
  const osPanels = document.querySelectorAll('.os-panel[data-os-panel]');
  osTabs.forEach((tab) => {
    tab.addEventListener('click', () => {
      if (isPlaying) setPlaying(false);
      const os = tab.getAttribute('data-os');
      osTabs.forEach((t) => {
        const active = t === tab;
        t.classList.toggle('active', active);
        t.setAttribute('aria-selected', String(active));
      });
      osPanels.forEach((p) => p.classList.toggle('active', p.getAttribute('data-os-panel') === os));
    });
  });

  const distroCommands = {
    arch: {
      label: '1. Arch Linux dependencies (Rust 1.88+)',
      cmd: 'sudo pacman -S --needed base-devel git rustup libxkbcommon mesa wayland libx11\nrustup default stable',
    },
    debian: {
      label: '1. Debian / Ubuntu / Linux Mint dependencies',
      cmd: 'sudo apt install build-essential git curl pkg-config libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev\ncurl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh',
    },
    fedora: {
      label: '1. Fedora dependencies',
      cmd: 'sudo dnf install gcc gcc-c++ make git curl pkgconf-pkg-config libxkbcommon-devel mesa-libGL-devel mesa-libEGL-devel wayland-devel libX11-devel\ncurl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh',
    },
    opensuse: {
      label: '1. openSUSE dependencies',
      cmd: 'sudo zypper install gcc gcc-c++ make git curl pkg-config libxkbcommon-devel Mesa-libGL-devel Mesa-libEGL-devel wayland-devel libX11-devel\ncurl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh',
    },
    gentoo: {
      label: '1. Gentoo dependencies',
      cmd: 'sudo emerge --ask --noreplace dev-vcs/git net-misc/curl x11-libs/libxkbcommon media-libs/mesa dev-libs/wayland x11-libs/libX11\ncurl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh',
    },
    void: {
      label: '1. Void Linux dependencies',
      cmd: 'sudo xbps-install -S base-devel git curl pkg-config libxkbcommon-devel MesaLib-devel wayland-devel libX11-devel\ncurl --proto \'=https\' --tlsv1.2 -sSf https://sh.rustup.rs | sh',
    },
  };

  const distroPills = document.querySelectorAll('.distro-pill[data-distro]');
  const distroCode = document.getElementById('distroCode');
  const distroCmdLabel = document.getElementById('distroCmdLabel');
  distroPills.forEach((pill) => {
    pill.addEventListener('click', () => {
      if (isPlaying) setPlaying(false);
      const info = distroCommands[pill.getAttribute('data-distro')];
      if (!info) return;
      distroPills.forEach((p) => p.classList.toggle('active', p === pill));
      if (distroCmdLabel) distroCmdLabel.textContent = info.label;
      if (distroCode) distroCode.textContent = info.cmd;
    });
  });

  document.querySelectorAll('.copy-btn[data-copy-target]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const el = document.getElementById(btn.getAttribute('data-copy-target') || '');
      if (!el) return;
      try {
        await navigator.clipboard.writeText(el.textContent || '');
        const orig = btn.textContent;
        btn.textContent = 'Copied!';
        setTimeout(() => (btn.textContent = orig), 1500);
      } catch {
        btn.textContent = 'Copy';
      }
    });
  });

  // Init
  updateSlideClasses();
})();
