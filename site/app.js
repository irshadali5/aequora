(function() {
  'use strict';

  const DATA = window.AEQUORA_DOCS_DATA || { tiers: [], docs: [] };
  let currentDocId = null;

  // DOM Elements
  const sidebarNav = document.getElementById('sidebar-nav');
  const mainContent = document.getElementById('main-content');
  const tocList = document.getElementById('toc-list');
  const progressBar = document.getElementById('progress-bar');
  const searchModalBackdrop = document.getElementById('search-modal-backdrop');
  const searchInput = document.getElementById('search-modal-input');
  const searchResults = document.getElementById('search-results');
  const searchTrigger = document.getElementById('search-trigger');
  const searchClose = document.getElementById('search-modal-close');
  const themeToggle = document.getElementById('theme-toggle');
  const mobileMenuBtn = document.getElementById('mobile-menu-btn');
  const sidebar = document.getElementById('sidebar');

  // Theme Handling
  function initTheme() {
    const saved = localStorage.getItem('aequora-theme') || 'dark';
    document.documentElement.setAttribute('data-theme', saved);
    updateThemeIcon(saved);
  }

  function toggleTheme() {
    const current = document.documentElement.getAttribute('data-theme') || 'dark';
    const next = current === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    localStorage.setItem('aequora-theme', next);
    updateThemeIcon(next);
  }

  function updateThemeIcon(theme) {
    if (themeToggle) {
      themeToggle.innerHTML = theme === 'dark' ? '☀️' : '🌙';
    }
  }

  if (themeToggle) {
    themeToggle.addEventListener('click', toggleTheme);
  }
  initTheme();

  // Mobile Menu Toggle
  if (mobileMenuBtn && sidebar) {
    mobileMenuBtn.addEventListener('click', () => {
      sidebar.classList.toggle('open');
    });
    // Close sidebar on doc click on mobile
    sidebar.addEventListener('click', (e) => {
      if (e.target.closest('.doc-nav-item')) {
        sidebar.classList.remove('open');
      }
    });
  }

  // Build Sidebar Navigation
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
        <span class="tier-icon">${tier.icon}</span>
        <span>${tier.name}</span>
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
        itemsEl.appendChild(a);
      });

      groupEl.appendChild(titleEl);
      groupEl.appendChild(itemsEl);
      sidebarNav.appendChild(groupEl);
    });
  }

  // Sidebar live search filter
  const sidebarSearchInput = document.getElementById('sidebar-search-input');
  if (sidebarSearchInput) {
    sidebarSearchInput.addEventListener('input', (e) => {
      renderSidebar(e.target.value);
    });
  }

  // Helper Escape HTML
  function escapeHtml(str) {
    if (!str) return '';
    return str.replace(/&/g, '&amp;')
              .replace(/</g, '&lt;')
              .replace(/>/g, '&gt;')
              .replace(/"/g, '&quot;')
              .replace(/'/g, '&#039;');
  }

  // Minimal Markdown Parser & Formatter
  function parseMarkdown(md) {
    if (!md) return '';

    // Convert Github Callouts
    md = md.replace(/^>\s*\[!NOTE\]\s*(.+)$/gm, '<div class="callout note"><strong>Note:</strong> $1</div>');
    md = md.replace(/^>\s*\[!TIP\]\s*(.+)$/gm, '<div class="callout tip"><strong>Tip:</strong> $1</div>');
    md = md.replace(/^>\s*\[!IMPORTANT\]\s*(.+)$/gm, '<div class="callout important"><strong>Important:</strong> $1</div>');
    md = md.replace(/^>\s*\[!WARNING\]\s*(.+)$/gm, '<div class="callout warning"><strong>Warning:</strong> $1</div>');
    md = md.replace(/^>\s*\[!CAUTION\]\s*(.+)$/gm, '<div class="callout caution"><strong>Caution:</strong> $1</div>');

    // Use marked if available, else standard regex renderer
    if (typeof marked !== 'undefined' && marked.parse) {
      let html = marked.parse(md);
      // Highlight invariants
      html = html.replace(/(AEQ-INV-[A-Z0-9]+)/g, '<span class="inv-tag">$1</span>');
      return html;
    }

    // Fallback basic renderer
    let out = [];
    let inCode = false;
    let codeLang = '';
    let codeBuffer = [];

    const lines = md.split('\n');
    for (let line of lines) {
      if (line.startsWith('```')) {
        if (!inCode) {
          inCode = true;
          codeLang = line.slice(3).trim();
          codeBuffer = [];
        } else {
          inCode = false;
          out.push(`<pre><button class="copy-code-btn" onclick="navigator.clipboard.writeText(this.parentElement.querySelector('code').innerText);this.innerText='Copied!';setTimeout(()=>this.innerText='Copy',1500)">Copy</button><code class="language-${codeLang}">${escapeHtml(codeBuffer.join('\n'))}</code></pre>`);
        }
        continue;
      }

      if (inCode) {
        codeBuffer.push(line);
        continue;
      }

      // Headings
      if (line.startsWith('#### ')) {
        const h = line.slice(5).trim();
        const slug = h.toLowerCase().replace(/[^a-z0-9]+/g, '-');
        out.push(`<h4 id="${slug}">${escapeHtml(h)}</h4>`);
      } else if (line.startsWith('### ')) {
        const h = line.slice(4).trim();
        const slug = h.toLowerCase().replace(/[^a-z0-9]+/g, '-');
        out.push(`<h3 id="${slug}">${escapeHtml(h)}</h3>`);
      } else if (line.startsWith('## ')) {
        const h = line.slice(3).trim();
        const slug = h.toLowerCase().replace(/[^a-z0-9]+/g, '-');
        out.push(`<h2 id="${slug}">${escapeHtml(h)}</h2>`);
      } else if (line.startsWith('# ')) {
        const h = line.slice(2).trim();
        const slug = h.toLowerCase().replace(/[^a-z0-9]+/g, '-');
        out.push(`<h1 id="${slug}">${escapeHtml(h)}</h1>`);
      } else if (line.startsWith('> ')) {
        out.push(`<blockquote>${escapeHtml(line.slice(2))}</blockquote>`);
      } else if (line.startsWith('- ') || line.startsWith('* ')) {
        out.push(`<li>${escapeHtml(line.slice(2))}</li>`);
      } else if (line.trim() === '---') {
        out.push('<hr style="border:0;border-top:1px solid var(--border-color);margin:24px 0;">');
      } else if (line.trim()) {
        let p = escapeHtml(line);
        // Inline code
        p = p.replace(/`([^`]+)`/g, '<code>$1</code>');
        // Bold
        p = p.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
        // Invariants
        p = p.replace(/(AEQ-INV-[A-Z0-9]+)/g, '<span class="inv-tag">$1</span>');
        out.push(`<p>${p}</p>`);
      }
    }

    return out.join('\n');
  }

  // Render Document
  function renderDocument(docId) {
    const doc = DATA.docs.find(d => d.id === docId) || DATA.docs[0];
    if (!doc) return;

    currentDocId = doc.id;
    window.location.hash = `#/${doc.id}`;
    window.scrollTo({ top: 0, behavior: 'instant' });

    // Update active sidebar items
    renderSidebar(sidebarSearchInput ? sidebarSearchInput.value : '');

    // Previous & Next navigation
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
    mainContent.innerHTML = `
      <div class="doc-banner">
        <div class="doc-meta-tags">
          <span class="meta-pill tier">${doc.tierIcon} Tier ${doc.tierId.split('-')[1]}: ${doc.tierName}</span>
          <span class="meta-pill">Part ${doc.num}</span>
          ${doc.invariants.length > 0 ? `<span class="meta-pill invariants">${doc.invariants.length} Invariants</span>` : ''}
        </div>
        <h1 class="doc-main-title">${escapeHtml(doc.title)}</h1>
        <div class="doc-stats">
          <span>⏱️ ~${doc.readTime} min read</span>
          <span>•</span>
          <span>📝 ${doc.words.toLocaleString()} words</span>
          <span>•</span>
          <span>📄 ${doc.file}</span>
        </div>
      </div>

      ${invariantsHtml}

      <div class="markdown-body" id="doc-markdown-body">
        ${parseMarkdown(doc.content)}
      </div>

      <div class="doc-footer-nav">
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
      </div>
    `;

    // Attach copy buttons to all pre code elements
    document.querySelectorAll('.markdown-body pre').forEach(pre => {
      if (!pre.querySelector('.copy-code-btn')) {
        const btn = document.createElement('button');
        btn.className = 'copy-code-btn';
        btn.innerText = 'Copy';
        btn.onclick = () => {
          const code = pre.querySelector('code')?.innerText || pre.innerText;
          navigator.clipboard.writeText(code);
          btn.innerText = 'Copied!';
          setTimeout(() => { btn.innerText = 'Copy'; }, 1500);
        };
        pre.appendChild(btn);
      }
    });

    // Build TOC
    renderTOC(doc);
  }

  // Render Table of Contents
  function renderTOC(doc) {
    if (!tocList) return;
    tocList.innerHTML = '';

    const contentHeadings = document.querySelectorAll('.markdown-body h2, .markdown-body h3');
    if (contentHeadings.length === 0) {
      tocList.innerHTML = '<li style="color:var(--text-muted);font-size:12px;">No subheadings</li>';
      return;
    }

    contentHeadings.forEach((h, idx) => {
      if (!h.id) {
        h.id = 'heading-' + idx;
      }
      const li = document.createElement('li');
      const a = document.createElement('a');
      a.href = '#' + h.id;
      a.className = `toc-link ${h.tagName === 'H3' ? 'level-3' : 'level-2'}`;
      a.innerText = h.innerText;
      a.addEventListener('click', (e) => {
        e.preventDefault();
        h.scrollIntoView({ behavior: 'smooth' });
      });
      li.appendChild(a);
      tocList.appendChild(li);
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
        if (top <= 120) {
          activeId = h.id;
        }
      });

      document.querySelectorAll('.toc-link').forEach(link => {
        link.classList.toggle('active', link.getAttribute('href') === '#' + activeId);
      });
    };
  }

  // Search Modal
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

  // Keyboard shortcuts
  document.addEventListener('keydown', (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      openSearch();
    }
    if (e.key === 'Escape') {
      closeSearch();
    }
  });

  // Search Query Execution
  function runSearch(query) {
    if (!searchResults) return;
    searchResults.innerHTML = '';
    const q = query.toLowerCase().trim();

    if (!q) {
      searchResults.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-muted);font-size:13px;">Type to search 30 architecture specs and formal invariants...</div>';
      return;
    }

    const matches = [];
    DATA.docs.forEach(doc => {
      let score = 0;
      let snippet = '';

      if (doc.title.toLowerCase().includes(q)) {
        score += 50;
        snippet = `Document title match: <strong>${doc.title}</strong>`;
      }

      const invMatch = doc.invariants.find(inv => inv.toLowerCase().includes(q));
      if (invMatch) {
        score += 40;
        snippet = `Formal invariant match: <span class="inv-tag">${invMatch}</span>`;
      }

      const bodyIdx = doc.content.toLowerCase().indexOf(q);
      if (bodyIdx !== -1 && !snippet) {
        score += 20;
        const start = Math.max(0, bodyIdx - 40);
        const end = Math.min(doc.content.length, bodyIdx + 80);
        snippet = '...' + escapeHtml(doc.content.substring(start, end)) + '...';
      }

      if (score > 0) {
        matches.push({ doc, score, snippet });
      }
    });

    matches.sort((a, b) => b.score - a.score);

    if (matches.length === 0) {
      searchResults.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-muted);font-size:13px;">No results found for "' + escapeHtml(q) + '"</div>';
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

  // Routing on hash change
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
