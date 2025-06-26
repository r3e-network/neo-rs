// Neo-RS Documentation JavaScript
(function() {
    'use strict';

    // Documentation-specific functionality
    function initDocumentationFeatures() {
        initMethodTabs();
        initCodeCopyButtons();
        initTableOfContents();
        initSearchHighlight();
        initPrintOptimization();
    }

    // Method/installation tabs functionality
    function initMethodTabs() {
        const tabButtons = document.querySelectorAll('.method-tab');
        const tabContents = document.querySelectorAll('.method-content');

        tabButtons.forEach(button => {
            button.addEventListener('click', function() {
                const targetMethod = this.getAttribute('data-method');
                
                // Remove active class from all buttons and contents
                tabButtons.forEach(btn => btn.classList.remove('active'));
                tabContents.forEach(content => content.classList.remove('active'));
                
                // Add active class to clicked button and corresponding content
                this.classList.add('active');
                const targetContent = document.getElementById(targetMethod);
                if (targetContent) {
                    targetContent.classList.add('active');
                }

                // Track tab selection for analytics
                if (typeof gtag !== 'undefined') {
                    gtag('event', 'documentation_tab_click', {
                        event_category: 'documentation',
                        event_label: targetMethod
                    });
                }
            });
        });
    }

    // Enhanced code copy functionality for documentation
    function initCodeCopyButtons() {
        document.querySelectorAll('.copy-btn').forEach(button => {
            button.addEventListener('click', function() {
                const copyId = this.getAttribute('data-copy');
                const codeElement = document.getElementById(copyId);
                
                if (codeElement) {
                    const text = codeElement.textContent.trim();
                    
                    // Use Clipboard API
                    if (navigator.clipboard) {
                        navigator.clipboard.writeText(text).then(() => {
                            showCopySuccess(this);
                        }).catch(() => {
                            fallbackCopyText(text);
                            showCopySuccess(this);
                        });
                    } else {
                        fallbackCopyText(text);
                        showCopySuccess(this);
                    }

                    // Track copy events for analytics
                    if (typeof gtag !== 'undefined') {
                        gtag('event', 'code_copy', {
                            event_category: 'documentation',
                            event_label: copyId
                        });
                    }
                }
            });
        });
    }

    // Fallback copy function
    function fallbackCopyText(text) {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.left = '-999999px';
        textArea.style.top = '-999999px';
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        
        try {
            document.execCommand('copy');
        } catch (err) {
            console.error('Failed to copy text: ', err);
        }
        
        document.body.removeChild(textArea);
    }

    // Show copy success feedback
    function showCopySuccess(button) {
        const originalText = button.textContent;
        button.textContent = 'Copied!';
        button.style.backgroundColor = '#28ca42';
        button.style.borderColor = '#28ca42';
        button.style.color = 'white';
        
        setTimeout(() => {
            button.textContent = originalText;
            button.style.backgroundColor = '';
            button.style.borderColor = '';
            button.style.color = '';
        }, 2000);
    }

    // Dynamic table of contents generation
    function initTableOfContents() {
        const tocContainer = document.querySelector('.docs-toc ul');
        if (!tocContainer) return;

        const headings = document.querySelectorAll('.docs-content h2, .docs-content h3');
        
        // Clear existing TOC (if any)
        tocContainer.innerHTML = '';

        headings.forEach((heading, index) => {
            // Add ID if not present
            if (!heading.id) {
                heading.id = heading.textContent
                    .toLowerCase()
                    .replace(/[^a-z0-9]+/g, '-')
                    .replace(/(^-|-$)/g, '');
            }

            // Create TOC entry
            const li = document.createElement('li');
            const a = document.createElement('a');
            a.href = `#${heading.id}`;
            a.textContent = heading.textContent;
            
            // Add class based on heading level
            if (heading.tagName === 'H3') {
                li.classList.add('toc-subsection');
            }
            
            li.appendChild(a);
            tocContainer.appendChild(li);

            // Add scroll spy functionality
            a.addEventListener('click', function(e) {
                e.preventDefault();
                const target = document.getElementById(heading.id);
                if (target) {
                    const offsetTop = target.offsetTop - 100; // Account for fixed header
                    window.scrollTo({
                        top: offsetTop,
                        behavior: 'smooth'
                    });
                }
            });
        });

        // Add scroll spy for active section highlighting
        initScrollSpy();
    }

    // Scroll spy for TOC active states
    function initScrollSpy() {
        const tocLinks = document.querySelectorAll('.docs-toc a');
        const sections = document.querySelectorAll('.docs-content h2, .docs-content h3');
        
        if (tocLinks.length === 0 || sections.length === 0) return;

        function updateActiveTocLink() {
            let activeSection = null;
            const scrollPosition = window.scrollY + 150; // Offset for header

            sections.forEach(section => {
                if (section.offsetTop <= scrollPosition) {
                    activeSection = section;
                }
            });

            // Remove active class from all links
            tocLinks.forEach(link => link.classList.remove('active'));

            // Add active class to current section link
            if (activeSection) {
                const activeLink = document.querySelector(`.docs-toc a[href="#${activeSection.id}"]`);
                if (activeLink) {
                    activeLink.classList.add('active');
                }
            }
        }

        // Throttled scroll handler
        let scrollTimeout;
        window.addEventListener('scroll', () => {
            if (scrollTimeout) {
                clearTimeout(scrollTimeout);
            }
            scrollTimeout = setTimeout(updateActiveTocLink, 10);
        });

        // Initial call
        updateActiveTocLink();
    }

    // Search highlighting functionality
    function initSearchHighlight() {
        const urlParams = new URLSearchParams(window.location.search);
        const searchTerm = urlParams.get('highlight');
        
        if (searchTerm) {
            highlightText(searchTerm);
        }
    }

    // Highlight text in the document
    function highlightText(searchTerm) {
        const walker = document.createTreeWalker(
            document.querySelector('.docs-content'),
            NodeFilter.SHOW_TEXT,
            null,
            false
        );

        const textNodes = [];
        let node;
        while (node = walker.nextNode()) {
            textNodes.push(node);
        }

        const regex = new RegExp(`(${escapeRegExp(searchTerm)})`, 'gi');
        
        textNodes.forEach(textNode => {
            if (textNode.textContent.match(regex)) {
                const parent = textNode.parentNode;
                const wrapper = document.createElement('span');
                wrapper.innerHTML = textNode.textContent.replace(regex, '<mark class="search-highlight">$1</mark>');
                parent.replaceChild(wrapper, textNode);
            }
        });

        // Scroll to first highlight
        const firstHighlight = document.querySelector('.search-highlight');
        if (firstHighlight) {
            firstHighlight.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
    }

    // Escape special regex characters
    function escapeRegExp(string) {
        return string.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }

    // Print optimization
    function initPrintOptimization() {
        // Add print styles dynamically
        const printStyles = `
            @media print {
                .docs-sidebar { display: none; }
                .docs-content { margin-left: 0; max-width: none; }
                .docs-nav { display: none; }
                .copy-btn { display: none; }
                .navbar { display: none; }
                .method-tabs { display: none; }
                .method-content:not(.active) { display: block !important; }
                .code-block { break-inside: avoid; }
                .docs-section { break-inside: avoid; }
                a[href]:after { content: " (" attr(href) ")"; }
                .docs-toc { display: none; }
            }
        `;

        const styleSheet = document.createElement('style');
        styleSheet.textContent = printStyles;
        document.head.appendChild(styleSheet);

        // Handle print button if exists
        const printButton = document.querySelector('.print-button');
        if (printButton) {
            printButton.addEventListener('click', () => {
                window.print();
            });
        }
    }

    // Sidebar toggle for mobile
    function initSidebarToggle() {
        const sidebarToggle = document.querySelector('.sidebar-toggle');
        const sidebar = document.querySelector('.docs-sidebar');
        
        if (sidebarToggle && sidebar) {
            sidebarToggle.addEventListener('click', () => {
                sidebar.classList.toggle('active');
                sidebarToggle.classList.toggle('active');
            });

            // Close sidebar when clicking outside
            document.addEventListener('click', (e) => {
                if (!sidebar.contains(e.target) && !sidebarToggle.contains(e.target)) {
                    sidebar.classList.remove('active');
                    sidebarToggle.classList.remove('active');
                }
            });
        }
    }

    // Feedback functionality
    function initFeedback() {
        const feedbackButtons = document.querySelectorAll('.feedback-button');
        
        feedbackButtons.forEach(button => {
            button.addEventListener('click', function() {
                const isHelpful = this.dataset.helpful === 'true';
                const pageUrl = window.location.pathname;
                
                // Track feedback
                if (typeof gtag !== 'undefined') {
                    gtag('event', 'documentation_feedback', {
                        event_category: 'documentation',
                        event_label: pageUrl,
                        value: isHelpful ? 1 : 0
                    });
                }

                // Show thank you message
                const feedbackContainer = this.parentElement;
                feedbackContainer.innerHTML = '<p class="feedback-thanks">Thank you for your feedback!</p>';
            });
        });
    }

    // Code syntax highlighting (simple version)
    function initSyntaxHighlighting() {
        const codeBlocks = document.querySelectorAll('pre code');
        
        codeBlocks.forEach(block => {
            const text = block.textContent;
            
            // Simple highlighting for common patterns
            let highlighted = text
                // Comments
                .replace(/(#.*$)/gm, '<span class="comment">$1</span>')
                // Strings
                .replace(/(".*?")/g, '<span class="string">$1</span>')
                .replace(/('.*?')/g, '<span class="string">$1</span>')
                // Keywords (basic set)
                .replace(/\b(cargo|docker|git|curl|sudo|brew|npm|cd|mkdir|cp|mv|rm)\b/g, '<span class="keyword">$1</span>')
                // URLs
                .replace(/(https?:\/\/[^\s]+)/g, '<span class="url">$1</span>')
                // Flags
                .replace(/(-{1,2}[a-zA-Z-]+)/g, '<span class="flag">$1</span>');
                
            block.innerHTML = highlighted;
        });
    }

    // Enhanced navigation for documentation
    function initDocNavigation() {
        const prevButton = document.querySelector('.nav-prev');
        const nextButton = document.querySelector('.nav-next');
        
        // Keyboard navigation
        document.addEventListener('keydown', (e) => {
            if (e.altKey) {
                if (e.key === 'ArrowLeft' && prevButton) {
                    e.preventDefault();
                    prevButton.click();
                } else if (e.key === 'ArrowRight' && nextButton) {
                    e.preventDefault();
                    nextButton.click();
                }
            }
        });
    }

    // Reading progress indicator
    function initReadingProgress() {
        const progressBar = document.querySelector('.reading-progress');
        if (!progressBar) return;

        function updateProgress() {
            const scrollTop = window.pageYOffset;
            const docHeight = document.documentElement.scrollHeight - window.innerHeight;
            const progress = (scrollTop / docHeight) * 100;
            
            progressBar.style.width = Math.min(progress, 100) + '%';
        }

        window.addEventListener('scroll', updateProgress);
        updateProgress(); // Initial call
    }

    // Initialize all documentation features
    function init() {
        initDocumentationFeatures();
        initSidebarToggle();
        initFeedback();
        initSyntaxHighlighting();
        initDocNavigation();
        initReadingProgress();
    }

    // Initialize when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Export utilities for debugging
    window.DocsUtils = {
        highlightText,
        showCopySuccess,
        updateActiveTocLink: initScrollSpy
    };

})();