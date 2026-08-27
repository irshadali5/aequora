(function() {
  'use strict';

  const DATA = window.AEQUORA_DOCS_DATA || { tiers: [], docs: [] };
  let currentDocId = null;

  // DOM Elements
  const sidebar = document.getElementById('sidebar');
  const sidebarNav = document.getElementById('sidebar-nav');
  const sidebarBackdrop = document.getElementById('sidebar-backdrop');
  const sidebarCloseBtn = document.getElementById('sidebar-close-btn');
  const sidebarSearchInput = document.getElementById('sidebar-search-input');
  const sidebarSearchClear = document.getElementById('sidebar-search-clear');
  const mobileMenuBtn = document.getElementById('mobile-menu-btn');

  const mainContent = document.getElementById('main-content');
  const tocList = document.getElementById('toc-list');
  const progressBar = document.getElementById('progress-bar');
  const scrollTopBtn = document.getElementById('scroll-top-btn');

  const mobileTocBtn = document.getElementById('mobile-toc-btn');
  const mobileTocCount = document.getElementById('mobile-toc-count');
  const mobileTocSheet = document.getElementById('mobile-toc-sheet');
  const mobileTocBackdrop = document.getElementById('mobile-toc-backdrop');
  const mobileTocClose = document.getElementById('mobile-toc-close');
  const mobileTocList = document.getElementById('mobile-toc-list');

  const searchModalBackdrop = document.getElementById('search-modal-backdrop');
  const searchInput = document.getElementById('search-modal-input');
  const searchResults = document.getElementById('search-results');
  const searchTrigger = document.getElementById('search-trigger');
  const searchClose = document.getElementById('search-modal-close');

  const themeToggle = document.getElementById('theme-toggle');
  const themeIconInner = document.getElementById('theme-icon-inner');
  const metaThemeColor = document.getElementById('meta-theme-color');

  // =========================================================================
  // Theme Management
  // =========================================================================
  function initTheme() {
    const saved = localStorage.getItem('aequora-theme') || 'dark';
    applyTheme(saved);
  }

  function applyTheme(theme) {
    document.documentElement.setAttribute('data-theme', theme);
    localStorage.setItem('aequora-theme', theme);
    if (themeIconInner) {
      themeIconInner.textContent = theme === 'dark' ? '☀️' : '🌙';
    }
    if (metaThemeColor) {
      metaThemeColor.setAttribute('content', theme === 'dark' ? '#090d16' : '#f8fafc');
    }
  }

  function toggleTheme() {
    const current = document.documentElement.getAttribute('data-theme') || 'dark';
    const next = current === 'dark' ? 'light' : 'dark';
    applyTheme(next);
  }

  if (themeToggle) {
    themeToggle.addEventListener('click', toggleTheme);
  }
  initTheme();

  // =========================================================================
  // Mobile Sidebar Drawer Management
  // =========================================================================
  function openSidebar() {
    if (sidebar) sidebar.classList.add('open');
    if (sidebarBackdrop) sidebarBackdrop.classList.add('active');
    document.body.style.overflow = window.innerWidth <= 860 ? 'hidden' : '';
  }

  function closeSidebar() {
    if (sidebar) sidebar.classList.remove('open');
    if (sidebarBackdrop) sidebarBackdrop.classList.remove('active');
    document.body.style.overflow = '';
  }

  if (mobileMenuBtn) {
    mobileMenuBtn.addEventListener('click', () => {
      if (sidebar && sidebar.classList.contains('open')) {
        closeSidebar();
      } else {
        openSidebar();
      }
    });
  }

  if (sidebarCloseBtn) {
    sidebarCloseBtn.addEventListener('click', closeSidebar);
  }

  if (sidebarBackdrop) {
    sidebarBackdrop.addEventListener('click', closeSidebar);
  }

  // =========================================================================
  // Mobile Table of Contents Bottom Sheet Management
  // =========================================================================
  function openMobileToc() {
    if (mobileTocSheet) mobileTocSheet.classList.add('open');
    if (mobileTocBackdrop) mobileTocBackdrop.classList.add('active');
  }

  function closeMobileToc() {
    if (mobileTocSheet) mobileTocSheet.classList.remove('open');
    if (mobileTocBackdrop) mobileTocBackdrop.classList.remove('active');
  }

  if (mobileTocBtn) mobileTocBtn.addEventListener('click', openMobileToc);
  if (mobileTocClose) mobileTocClose.addEventListener('click', closeMobileToc);
  if (mobileTocBackdrop) mobileTocBackdrop.addEventListener('click', closeMobileToc);

  // =========================================================================
  // Scroll to Top Button
  // =========================================================================
  if (scrollTopBtn) {
    scrollTopBtn.addEventListener('click', () => {
      window.scrollTo({ top: 0, behavior: 'smooth' });
    });
  }

  // =========================================================================
  // Sidebar Spec Navigation
  // =========================================================================
  function renderSidebar(filterText = '') {
    if (!sidebarNav) return;
    sidebarNav.innerHTML = '';
    const q = filterText.toLowerCase().trim();

    DATA.tiers.forEach(tier => {
      const tierDocs = DATA.docs.filter(d => {
        if (d.tierId !== tier.id) return false;
        if (!q) return true;
        return d.title.toLowerCase().includes(q) || 
               d.num.includes(q) || 
               d.invariants.some(inv => inv.toLowerCase().includes(q));
      });

      if (tierDocs.length === 0) return;

      const groupEl = document.createElement('div');
      groupEl.className = 'tier-group';

      const titleEl = document.createElement('div');
      titleEl.className = 'tier-title';
      titleEl.innerHTML = `
        <div class="tier-title-left">
          <span class="tier-icon">${tier.icon}</span>
          <span class="tier-name-text">${tier.name}</span>
        </div>
        <span class="tier-toggle-icon">▼</span>
      `;
      titleEl.addEventListener('click', () => {
        groupEl.classList.toggle('collapsed');
      });

      const itemsEl = document.createElement('div');
      itemsEl.className = 'tier-items';

      tierDocs.forEach(d => {
        const a = document.createElement('a');
        a.href = `#/${d.id}`;
        a.className = `doc-nav-item ${d.id === currentDocId ? 'active' : ''}`;
        a.innerHTML = `
          <span class="doc-nav-num">${d.num}</span>
          <span class="doc-nav-title" title="${escapeHtml(d.title)}">${escapeHtml(d.title)}</span>
          ${d.invariants.length > 0 ? `<span class="doc-nav-inv-badge">${d.invariants.length}</span>` : ''}
        `;
        a.addEventListener('click', () => {
          if (window.innerWidth <= 860) {
            closeSidebar();
          }
        });
        itemsEl.appendChild(a);
      });

      groupEl.appendChild(titleEl);
      groupEl.appendChild(itemsEl);
      sidebarNav.appendChild(groupEl);
    });
  }

  // Sidebar live search filter
  if (sidebarSearchInput) {
    sidebarSearchInput.addEventListener('input', (e) => {
      const val = e.target.value;
      if (sidebarSearchClear) {
        sidebarSearchClear.style.display = val ? 'block' : 'none';
      }
      renderSidebar(val);
    });
  }

  if (sidebarSearchClear) {
    sidebarSearchClear.addEventListener('click', () => {
      if (sidebarSearchInput) {
        sidebarSearchInput.value = '';
        sidebarSearchClear.style.display = 'none';
        sidebarSearchInput.focus();
        renderSidebar('');
      }
    });
  }

  // =========================================================================
  // Markdown & Document Rendering
  // =========================================================================
  function escapeHtml(str) {
    if (!str) return '';
    return str
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;')
      .replace(/'/g, '&#039;');
  }

  function parseMarkdown(md) {
    if (!md) return '';

    // Convert Github Callouts
    md = md.replace(/^>\s*\[!NOTE\]\s*(.+)$/gm, '<div class="callout note"><strong>Note:</strong> $1</div>');
    md = md.replace(/^>\s*\[!TIP\]\s*(.+)$/gm, '<div class="callout tip"><strong>Tip:</strong> $1</div>');
    md = md.replace(/^>\s*\[!IMPORTANT\]\s*(.+)$/gm, '<div class="callout important"><strong>Important:</strong> $1</div>');
    md = md.replace(/^>\s*\[!WARNING\]\s*(.+)$/gm, '<div class="callout warning"><strong>Warning:</strong> $1</div>');
    md = md.replace(/^>\s*\[!CAUTION\]\s*(.+)$/gm, '<div class="callout caution"><strong>Caution:</strong> $1</div>');

    // Use marked if available
    if (typeof marked !== 'undefined' && marked.parse) {
      let html = marked.parse(md);
      // Highlight formal invariants
      html = html.replace(/(AEQ-INV-[A-Z0-9]+)/g, '<span class="inv-tag">$1</span>');
      return html;
    }

    // Basic fallback renderer
    return md.replace(/\n\n/g, '<p></p>');
  }

  function renderDocument(docId) {
    const doc = DATA.docs.find(d => d.id === docId) || DATA.docs[0];
    if (!doc) return;

    currentDocId = doc.id;
    document.title = `Part ${doc.num}: ${doc.title} — Aequora Architecture`;

    // Re-render sidebar to highlight active doc
    renderSidebar(sidebarSearchInput ? sidebarSearchInput.value : '');

    // Find Previous & Next Docs for footer navigation
    const currentIndex = DATA.docs.findIndex(d => d.id === doc.id);
    const prevDoc = currentIndex > 0 ? DATA.docs[currentIndex - 1] : null;
    const nextDoc = currentIndex < DATA.docs.length - 1 ? DATA.docs[currentIndex + 1] : null;

    // Render Invariant Badges Box
    const invariantsHtml = doc.invariants.length > 0 ? `
      <div class="invariants-box">
        <div class="invariants-box-title">
          <span>⚡</span>
          <span>Formal Architecture Invariants (${doc.invariants.length})</span>
        </div>
        <div class="invariants-tags">
          ${doc.invariants.map(inv => `<span class="inv-tag">${inv}</span>`).join('')}
        </div>
      </div>
    ` : '';

    // Render Main Content
    if (mainContent) {
      mainContent.innerHTML = `
        <article class="doc-wrapper">
          <header class="doc-banner">
            <div class="doc-meta-tags">
              <span class="meta-pill tier">${doc.tierIcon} Tier ${doc.tierId.split('-')[1]}: ${doc.tierName}</span>
              <span class="meta-pill">Part ${doc.num}</span>
              ${doc.invariants.length > 0 ? `<span class="meta-pill invariants">${doc.invariants.length} Invariants</span>` : ''}
            </div>
            <h1 class="doc-main-title">${escapeHtml(doc.title)}</h1>
            <div class="doc-stats">
              <span class="doc-stat-item">⏱️ ~${doc.readTime} min read</span>
              <span>•</span>
              <span class="doc-stat-item">📝 ${doc.words.toLocaleString()} words</span>
              <span>•</span>
              <span class="doc-stat-item">📄 ${doc.file}</span>
            </div>
          </header>

          ${invariantsHtml}

          <div class="markdown-body" id="doc-markdown-body">
            ${parseMarkdown(doc.content)}
          </div>

          <footer class="doc-footer-nav">
            ${prevDoc ? `
              <a href="#/${prevDoc.id}" class="footer-nav-card">
                <span class="footer-nav-label">← Previous Part ${prevDoc.num}</span>
                <span class="footer-nav-title">${escapeHtml(prevDoc.title)}</span>
              </a>
            ` : '<div></div>'}
            ${nextDoc ? `
              <a href="#/${nextDoc.id}" class="footer-nav-card" style="text-align: right;">
                <span class="footer-nav-label">Next Part ${nextDoc.num} →</span>
                <span class="footer-nav-title">${escapeHtml(nextDoc.title)}</span>
              </a>
            ` : '<div></div>'}
          </footer>
        </article>
      `;

      // Wrap all table elements in .table-container for mobile responsiveness
      mainContent.querySelectorAll('.markdown-body table').forEach(table => {
        if (!table.parentElement.classList.contains('table-container')) {
          const wrapper = document.createElement('div');
          wrapper.className = 'table-container';
          table.parentNode.insertBefore(wrapper, table);
          wrapper.appendChild(table);
        }
      });

      // Attach copy buttons to all pre code elements
      mainContent.querySelectorAll('.markdown-body pre').forEach(pre => {
        if (!pre.querySelector('.copy-code-btn')) {
          const btn = document.createElement('button');
          btn.className = 'copy-code-btn';
          btn.innerText = 'Copy';
          btn.setAttribute('aria-label', 'Copy code snippet');
          btn.onclick = () => {
            const code = pre.querySelector('code')?.innerText || pre.innerText;
            navigator.clipboard.writeText(code);
            btn.innerText = 'Copied!';
            setTimeout(() => { btn.innerText = 'Copy'; }, 1500);
          };
          pre.appendChild(btn);
        }
      });
    }

    // Scroll window back to top on doc transition
    window.scrollTo(0, 0);

    // Build Table of Contents
    renderTOC();
  }

  // =========================================================================
  // Table of Contents (Desktop & Mobile Bottom Sheet)
  // =========================================================================
  function renderTOC() {
    const headings = document.querySelectorAll('.markdown-body h2, .markdown-body h3');
    
    if (mobileTocCount) {
      mobileTocCount.textContent = headings.length > 0 ? headings.length : '';
    }

    if (tocList) tocList.innerHTML = '';
    if (mobileTocList) mobileTocList.innerHTML = '';

    if (headings.length === 0) {
      if (tocList) tocList.innerHTML = '<li style="color:var(--text-muted);font-size:12px;">No subheadings</li>';
      if (mobileTocList) mobileTocList.innerHTML = '<li style="color:var(--text-muted);font-size:13px;padding:10px;">No subheadings in this specification.</li>';
      return;
    }

    headings.forEach((h, idx) => {
      if (!h.id) {
        h.id = 'section-' + idx;
      }
      
      // Desktop TOC Link
      if (tocList) {
        const li = document.createElement('li');
        const a = document.createElement('a');
        a.href = '#' + h.id;
        a.className = `toc-link ${h.tagName === 'H3' ? 'level-3' : 'level-2'}`;
        a.textContent = h.textContent.trim();
        a.addEventListener('click', (e) => {
          e.preventDefault();
          h.scrollIntoView({ behavior: 'smooth' });
        });
        li.appendChild(a);
        tocList.appendChild(li);
      }

      // Mobile Bottom Sheet TOC Link
      if (mobileTocList) {
        const li = document.createElement('li');
        const a = document.createElement('a');
        a.href = '#' + h.id;
        a.className = `mobile-toc-item ${h.tagName === 'H3' ? 'level-3' : 'level-2'}`;
        a.textContent = h.textContent.trim();
        a.addEventListener('click', (e) => {
          e.preventDefault();
          closeMobileToc();
          h.scrollIntoView({ behavior: 'smooth' });
        });
        li.appendChild(a);
        mobileTocList.appendChild(li);
      }
    });

    setupScrollSpy();
  }

  // Scroll Spy for TOC & Progress Bar
  function setupScrollSpy() {
    window.onscroll = () => {
      // Progress Bar
      const winScroll = document.documentElement.scrollTop || document.body.scrollTop;
      const height = document.documentElement.scrollHeight - document.documentElement.clientHeight;
      const scrolled = height > 0 ? (winScroll / height) * 100 : 0;
      if (progressBar) {
        progressBar.style.width = scrolled + '%';
      }

      // TOC active link spy
      const headings = document.querySelectorAll('.markdown-body h2, .markdown-body h3');
      let activeId = '';
      headings.forEach(h => {
        const top = h.getBoundingClientRect().top;
        if (top <= 140) {
          activeId = h.id;
        }
      });

      document.querySelectorAll('.toc-link').forEach(link => {
        link.classList.toggle('active', link.getAttribute('href') === '#' + activeId);
      });

      document.querySelectorAll('.mobile-toc-item').forEach(link => {
        link.classList.toggle('active', link.getAttribute('href') === '#' + activeId);
      });
    };
  }

  // =========================================================================
  // Search Modal
  // =========================================================================
  function openSearch() {
    if (!searchModalBackdrop) return;
    searchModalBackdrop.classList.add('open');
    if (searchInput) {
      searchInput.value = '';
      searchInput.focus();
      runSearch('');
    }
  }

  function closeSearch() {
    if (!searchModalBackdrop) return;
    searchModalBackdrop.classList.remove('open');
  }

  if (searchTrigger) searchTrigger.addEventListener('click', openSearch);
  if (searchClose) searchClose.addEventListener('click', closeSearch);

  if (searchModalBackdrop) {
    searchModalBackdrop.addEventListener('click', (e) => {
      if (e.target === searchModalBackdrop) closeSearch();
    });
  }

  // Keyboard Shortcuts (⌘K / Ctrl+K and Escape)
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      openSearch();
    }
    if (e.key === 'Escape') {
      closeSearch();
      closeSidebar();
      closeMobileToc();
    }
  });

  // Search Query Execution
  function runSearch(query) {
    if (!searchResults) return;
    searchResults.innerHTML = '';
    const q = query.toLowerCase().trim();

    if (!q) {
      searchResults.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-muted);font-size:13px;">Type to search 30 architecture specifications, formal invariants, and protocols...</div>';
      return;
    }

    const matches = [];
    DATA.docs.forEach(doc => {
      let score = 0;
      let snippet = '';

      if (doc.title.toLowerCase().includes(q)) {
        score += 60;
        snippet = `Title match: <strong>${escapeHtml(doc.title)}</strong>`;
      }

      const invMatch = doc.invariants.find(inv => inv.toLowerCase().includes(q));
      if (invMatch) {
        score += 50;
        snippet = `Formal invariant match: <span class="inv-tag">${escapeHtml(invMatch)}</span>`;
      }

      const bodyIdx = doc.content.toLowerCase().indexOf(q);
      if (bodyIdx !== -1 && !snippet) {
        score += 25;
        const start = Math.max(0, bodyIdx - 35);
        const end = Math.min(doc.content.length, bodyIdx + 75);
        snippet = '...' + escapeHtml(doc.content.substring(start, end)) + '...';
      }

      if (score > 0) {
        matches.push({ doc, score, snippet });
      }
    });

    matches.sort((a, b) => b.score - a.score);

    if (matches.length === 0) {
      searchResults.innerHTML = `<div style="padding:20px;text-align:center;color:var(--text-muted);font-size:13px;">No results found for "${escapeHtml(q)}"</div>`;
      return;
    }

    matches.slice(0, 10).forEach(m => {
      const a = document.createElement('a');
      a.href = `#/${m.doc.id}`;
      a.className = 'search-result-item';
      a.innerHTML = `
        <div class="search-result-title">Part ${m.doc.num}: ${escapeHtml(m.doc.title)}</div>
        <div class="search-result-snippet">${m.snippet}</div>
      `;
      a.addEventListener('click', () => {
        closeSearch();
      });
      searchResults.appendChild(a);
    });
  }

  if (searchInput) {
    searchInput.addEventListener('input', (e) => {
      runSearch(e.target.value);
    });
  }

  // =========================================================================
  // Hash Routing
  // =========================================================================
  function handleHash() {
    const hash = window.location.hash.replace(/^#\/?/, '');
    if (hash && hash.startsWith('part-')) {
      renderDocument(hash);
    } else {
      renderDocument(DATA.docs[0]?.id || 'part-01');
    }
  }

  window.addEventListener('hashchange', handleHash);

  // Initialize
  handleHash();

})();
