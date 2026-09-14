const http = require('http');
const fs = require('fs');
const path = require('path');

// Simple HTTP server to serve the dashboard and handle WebSocket connections
const PORT = 8080;
const HOST = 'localhost';

const htmlContent = fs.readFileSync(path.join(__dirname, 'index.html'), 'utf8');

const server = http.createServer((req, res) => {
    if (req.url === '/' || req.url === '/index.html') {
        res.writeHead(200, { 'Content-Type': 'text/html' });
        res.end(htmlContent);
    } else if (req.url === '/api/status') {
        // Mock status endpoint - in production this would connect to actual node
        const mockStatus = {
            currentBlock: Math.floor(Math.random() * 10000000),
            progressPercent: (Math.random() * 100).toFixed(2),
            blocksPerSec: (45 + Math.random() * 20).toFixed(1),
            peerCount: 80 + Math.floor(Math.random() * 20),
            isSyncing: true
        };
        
        res.writeHead(200, { 
            'Content-Type': 'application/json',
            'Access-Control-Allow-Origin': '*'
        });
        res.end(JSON.stringify(mockStatus));
    } else if (req.url === '/ws') {
        // WebSocket upgrade handler could go here
        res.writeHead(404);
        res.end('WebSocket not implemented in demo server');
    } else {
        res.writeHead(404);
        res.end('Not found');
    }
});

console.log(`
╔═══════════════════════════════════════════════════════════╗
║                                                           ║
║   Neo-N3 Sync Monitor Dashboard                         ║
║                                                           ║
║   Server running at:                                      ║
║     • HTTP: http://${HOST}:${PORT}/                            ║
║                                                           ║
║   Dashboard Preview Mode (Simulation)                     ║
║                                                           ║
║   Press Ctrl+C to stop                                    ║
║                                                           ║
╚═══════════════════════════════════════════════════════════╝
`);

server.listen(PORT, HOST, () => {
    console.log(`Dashboard accessible at http://${HOST}:${PORT}`);
});
