// Neo-RS Website JavaScript
(function() {
    'use strict';

    // Navigation functionality
    function initNavigation() {
        const navToggle = document.getElementById('nav-toggle');
        const navMenu = document.getElementById('nav-menu');
        
        if (navToggle && navMenu) {
            navToggle.addEventListener('click', function() {
                navMenu.classList.toggle('active');
                navToggle.classList.toggle('active');
            });
        }

        // Close mobile menu when clicking on a link
        const navLinks = document.querySelectorAll('.nav-link');
        navLinks.forEach(link => {
            link.addEventListener('click', function() {
                navMenu.classList.remove('active');
                navToggle.classList.remove('active');
            });
        });

        // Smooth scrolling for navigation links
        document.querySelectorAll('a[href^="#"]').forEach(anchor => {
            anchor.addEventListener('click', function(e) {
                e.preventDefault();
                const target = document.querySelector(this.getAttribute('href'));
                if (target) {
                    const offsetTop = target.offsetTop - 80; // Account for fixed navbar
                    window.scrollTo({
                        top: offsetTop,
                        behavior: 'smooth'
                    });
                }
            });
        });
    }

    // Tab functionality for installation section
    function initTabs() {
        const tabButtons = document.querySelectorAll('.tab-button');
        const tabContents = document.querySelectorAll('.tab-content');

        tabButtons.forEach(button => {
            button.addEventListener('click', function() {
                const targetTab = this.getAttribute('data-tab');
                
                // Remove active class from all buttons and contents
                tabButtons.forEach(btn => btn.classList.remove('active'));
                tabContents.forEach(content => content.classList.remove('active'));
                
                // Add active class to clicked button and corresponding content
                this.classList.add('active');
                const targetContent = document.getElementById(targetTab);
                if (targetContent) {
                    targetContent.classList.add('active');
                }
            });
        });
    }

    // Copy code functionality
    function initCodeCopy() {
        document.querySelectorAll('.copy-button').forEach(button => {
            button.addEventListener('click', function() {
                const codeId = this.getAttribute('data-copy');
                const codeElement = document.getElementById(codeId);
                
                if (codeElement) {
                    const text = codeElement.textContent;
                    
                    // Use Clipboard API if available
                    if (navigator.clipboard) {
                        navigator.clipboard.writeText(text).then(() => {
                            showCopyFeedback(this);
                        }).catch(() => {
                            // Fallback for older browsers
                            fallbackCopyText(text);
                            showCopyFeedback(this);
                        });
                    } else {
                        // Fallback for older browsers
                        fallbackCopyText(text);
                        showCopyFeedback(this);
                    }
                }
            });
        });
    }

    // Fallback copy function for older browsers
    function fallbackCopyText(text) {
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.opacity = '0';
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        
        try {
            document.execCommand('copy');
        } catch (err) {
        }
        
        document.body.removeChild(textArea);
    }

    // Show copy feedback
    function showCopyFeedback(button) {
        const originalHTML = button.innerHTML;
        button.innerHTML = `
            <svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="20,6 9,17 4,12"></polyline>
            </svg>
        `;
        button.style.color = '#28ca42';
        
        setTimeout(() => {
            button.innerHTML = originalHTML;
            button.style.color = '';
        }, 2000);
    }

    // Navbar scroll effect
    function initNavbarScroll() {
        const navbar = document.querySelector('.navbar');
        let lastScrollY = window.scrollY;
        
        window.addEventListener('scroll', () => {
            const currentScrollY = window.scrollY;
            
            if (currentScrollY > 100) {
                navbar.classList.add('scrolled');
            } else {
                navbar.classList.remove('scrolled');
            }
            
            // Hide navbar on scroll down, show on scroll up
            if (currentScrollY > lastScrollY && currentScrollY > 100) {
                navbar.style.transform = 'translateY(-100%)';
            } else {
                navbar.style.transform = 'translateY(0)';
            }
            
            lastScrollY = currentScrollY;
        });
    }

    // Intersection Observer for animations
    function initScrollAnimations() {
        const observerOptions = {
            threshold: 0.1,
            rootMargin: '0px 0px -50px 0px'
        };

        const observer = new IntersectionObserver((entries) => {
            entries.forEach(entry => {
                if (entry.isIntersecting) {
                    entry.target.classList.add('animate-in');
                }
            });
        }, observerOptions);

        // Observe elements that should animate
        document.querySelectorAll('.feature-card, .doc-card, .community-card, .step-card').forEach(el => {
            observer.observe(el);
        });
    }

    // Terminal animation
    function initTerminalAnimation() {
        const terminalLines = document.querySelectorAll('.terminal-line');
        
        // Add typing animation to terminal
        terminalLines.forEach((line, index) => {
            if (line.querySelector('.command')) {
                setTimeout(() => {
                    line.style.opacity = '1';
                    typeText(line.querySelector('.command'));
                }, index * 1000);
            } else if (line.querySelector('.output')) {
                setTimeout(() => {
                    line.style.opacity = '1';
                }, index * 1000);
            }
        });
    }

    // Type text animation
    function typeText(element) {
        const text = element.textContent;
        element.textContent = '';
        element.style.opacity = '1';
        
        let i = 0;
        const timer = setInterval(() => {
            element.textContent += text[i];
            i++;
            if (i >= text.length) {
                clearInterval(timer);
            }
        }, 50);
    }

    // Performance chart animation
    function initChartAnimation() {
        const observer = new IntersectionObserver((entries) => {
            entries.forEach(entry => {
                if (entry.isIntersecting) {
                    const bars = entry.target.querySelectorAll('.bar');
                    bars.forEach((bar, index) => {
                        setTimeout(() => {
                            bar.style.transform = 'scaleY(1)';
                        }, index * 200);
                    });
                }
            });
        }, { threshold: 0.5 });

        const chart = document.querySelector('.chart');
        if (chart) {
            // Set initial state
            chart.querySelectorAll('.bar').forEach(bar => {
                bar.style.transform = 'scaleY(0)';
                bar.style.transformOrigin = 'bottom';
                bar.style.transition = 'transform 0.6s ease-out';
            });
            
            observer.observe(chart);
        }
    }

    // Lazy loading for images
    function initLazyLoading() {
        const imageObserver = new IntersectionObserver((entries) => {
            entries.forEach(entry => {
                if (entry.isIntersecting) {
                    const img = entry.target;
                    img.src = img.dataset.src;
                    img.classList.remove('lazy');
                    imageObserver.unobserve(img);
                }
            });
        });

        document.querySelectorAll('img[data-src]').forEach(img => {
            imageObserver.observe(img);
        });
    }

    // Theme toggle (if implemented in future)
    function initThemeToggle() {
        const themeToggle = document.querySelector('.theme-toggle');
        if (themeToggle) {
            themeToggle.addEventListener('click', () => {
                document.body.classList.toggle('light-theme');
                localStorage.setItem('theme', 
                    document.body.classList.contains('light-theme') ? 'light' : 'dark'
                );
            });
            
            // Load saved theme
            const savedTheme = localStorage.getItem('theme');
            if (savedTheme === 'light') {
                document.body.classList.add('light-theme');
            }
        }
    }

    // Error handling for external links
    function initExternalLinks() {
        document.querySelectorAll('a[target="_blank"]').forEach(link => {
            link.addEventListener('click', function(e) {
                // Add analytics tracking or error handling here if needed
                try {
                    // Track external link clicks
                    if (typeof gtag !== 'undefined') {
                        gtag('event', 'click', {
                            event_category: 'external_link',
                            event_label: this.href
                        });
                    }
                } catch (error) {
                }
            });
        });
    }

    // Performance monitoring
    function initPerformanceMonitoring() {
        // Monitor Core Web Vitals
        if ('web-vital' in window) {
            import('web-vitals').then(({ getCLS, getFID, getFCP, getLCP, getTTFB }) => {
                getCLS((metric) => {
                    // Send to analytics service
                });
                getFID((metric) => {
                    // Send to analytics service
                });
                getFCP((metric) => {
                    // Send to analytics service
                });
                getLCP((metric) => {
                    // Send to analytics service
                });
                getTTFB((metric) => {
                    // Send to analytics service
                });
            });
        }
    }

    // Search functionality (placeholder for future implementation)
    function initSearch() {
        const searchInput = document.querySelector('.search-input');
        if (searchInput) {
            searchInput.addEventListener('input', debounce(function(e) {
                const query = e.target.value.toLowerCase();
                // Implement search logic here
                
            }, 300));
        }
    }

    // Utility function: debounce
    function debounce(func, wait) {
        let timeout;
        return function executedFunction(/* Implementation needed */args) {
            const later = () => {
                clearTimeout(timeout);
                func(/* Implementation needed */args);
            };
            clearTimeout(timeout);
            timeout = setTimeout(later, wait);
        };
    }

    // Service Worker registration
    function initServiceWorker() {
        if ('serviceWorker' in navigator) {
            window.addEventListener('load', () => {
                navigator.serviceWorker.register('/sw.js')
                    .then(registration => {
                        // Service worker registered successfully
                    })
                    .catch(registrationError => {
                        // Service worker registration failed
                    });
            });
        }
    }

    // Initialize all functionality
    function init() {
        // Core functionality
        initNavigation();
        initTabs();
        initCodeCopy();
        initNavbarScroll();
        
        // Animations and visual effects
        initScrollAnimations();
        initTerminalAnimation();
        initChartAnimation();
        
        // Enhanced features
        initLazyLoading();
        initThemeToggle();
        initExternalLinks();
        initSearch();
        
        // Performance and PWA
        initPerformanceMonitoring();
        initServiceWorker();
        
        // Add loaded class to body for CSS animations
        document.body.classList.add('loaded');
    }

    // Initialize when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }

    // Handle page visibility changes
    document.addEventListener('visibilitychange', () => {
        if (document.hidden) {
            // Page is hidden - pause animations, etc.
            document.body.classList.add('page-hidden');
        } else {
            // Page is visible - resume animations, etc.
            document.body.classList.remove('page-hidden');
        }
    });

    // Global error handling
    window.addEventListener('error', (e) => {
        // Send error to monitoring service if configured
        // Error logging handled by monitoring service
    });

    // Expose utilities to global scope for debugging
    window.NeoRS = {
        utils: {
            debounce,
            typeText,
            showCopyFeedback,
            fallbackCopyText
        }
    };

})();