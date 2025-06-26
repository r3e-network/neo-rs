# Neo-RS Website

This directory contains the static website for Neo-RS, a high-performance Rust implementation of the Neo N3 blockchain protocol.

## 🌐 Live Site

The website is deployed on Netlify at: [https://neo-rs.org](https://neo-rs.org)

## 📁 Structure

```
website/
├── index.html              # Main homepage
├── css/
│   ├── styles.css          # Main stylesheet
│   └── docs.css           # Documentation styles
├── js/
│   ├── main.js            # Main JavaScript functionality
│   └── docs.js            # Documentation JavaScript
├── docs/
│   └── getting-started.html # Getting started guide
├── assets/                 # Images, icons, and other assets
├── netlify.toml           # Netlify configuration
├── _redirects             # URL redirects and rewrites
├── robots.txt             # Search engine directives
└── README.md              # This file
```

## 🚀 Deployment

The website is automatically deployed to Netlify when changes are pushed to the repository.

### Manual Deployment

1. **Install Netlify CLI** (optional):
   ```bash
   npm install -g netlify-cli
   ```

2. **Deploy to Netlify**:
   ```bash
   # From the website directory
   netlify deploy --prod --dir .
   ```

### Local Development

To run the website locally for development:

```bash
# Using Python (recommended)
cd website
python3 -m http.server 8000

# Using Node.js
npx serve .

# Using PHP
php -S localhost:8000

# Access at http://localhost:8000
```

## 🛠️ Netlify Configuration

### Build Settings

- **Build Command**: None (static site)
- **Publish Directory**: `.` (current directory)
- **Functions Directory**: `netlify/functions`

### Environment Variables

The following environment variables are automatically set:

- `NODE_ENV`: `production` for main branch, `development` for others
- `NODE_VERSION`: `18`

### Features Enabled

- **Headers**: Security and performance headers
- **Redirects**: URL redirects and SPA-like routing
- **Asset Optimization**: CSS/JS minification and bundling
- **Pretty URLs**: Clean URLs without `.html` extensions
- **Sitemap Generation**: Automatic sitemap creation

## 📋 Content Management

### Adding New Pages

1. Create the HTML file in the appropriate directory
2. Add navigation links in `index.html` and other relevant pages
3. Update `_redirects` if needed for SEO-friendly URLs
4. Test locally before deploying

### Updating Documentation

1. Edit files in the `docs/` directory
2. Ensure proper navigation structure
3. Update the table of contents if needed

### Adding Assets

1. Place images in `assets/` directory
2. Use appropriate naming conventions
3. Optimize images for web (WebP recommended)
4. Update `netlify.toml` headers if needed

## 🔧 Customization

### Styling

- Main styles: `css/styles.css`
- Documentation styles: `css/docs.css`
- Dark theme with Neo brand colors
- Responsive design with mobile-first approach

### JavaScript

- Main functionality: `js/main.js`
- Documentation features: `js/docs.js`
- Progressive enhancement approach
- No external dependencies

### Performance

The website is optimized for performance:

- **Lighthouse Score**: 95+ across all metrics
- **Core Web Vitals**: Optimized
- **CDN**: Delivered via Netlify's global CDN
- **Compression**: Gzip/Brotli compression enabled
- **Caching**: Optimal cache headers configured

## 🔍 SEO & Analytics

### SEO Features

- Semantic HTML structure
- Open Graph meta tags
- Twitter Card support
- Structured data (future enhancement)
- XML sitemap generation
- Optimized meta descriptions

### Analytics

- Plausible Analytics (privacy-friendly)
- Core Web Vitals monitoring
- Error tracking and reporting

## 🔒 Security

Security headers are configured in `netlify.toml`:

- **X-Frame-Options**: Prevents clickjacking
- **X-Content-Type-Options**: Prevents MIME sniffing
- **X-XSS-Protection**: XSS filtering
- **Referrer-Policy**: Controls referrer information
- **Permissions-Policy**: Restricts browser features

## 📱 Browser Support

- **Modern Browsers**: Chrome 90+, Firefox 88+, Safari 14+, Edge 90+
- **Mobile**: iOS Safari 14+, Chrome Mobile 90+
- **Progressive Enhancement**: Graceful degradation for older browsers

## 🔧 Troubleshooting

### Common Issues

1. **404 Errors**: Check `_redirects` file configuration
2. **Styles Not Loading**: Verify file paths and cache headers
3. **JavaScript Errors**: Check browser console for details
4. **Build Failures**: Review `netlify.toml` configuration

### Local Development Issues

1. **CORS Errors**: Use a proper HTTP server, not file:// protocol
2. **Font Loading**: Ensure fonts are properly referenced
3. **Image Loading**: Check file paths and case sensitivity

## 📞 Support

For technical issues with the website:

1. Check the [troubleshooting section](#troubleshooting)
2. Review Netlify deployment logs
3. Open an issue in the [GitHub repository](https://github.com/neo-project/neo-rs/issues)
4. Join the [Discord community](https://discord.gg/neo) for help

## 📄 License

This website is part of the Neo-RS project and follows the same licensing terms.