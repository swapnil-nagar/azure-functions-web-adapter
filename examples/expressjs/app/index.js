const express = require('express');
const app = express();
const port = process.env.PORT || 8080;

// JSON body parsing
app.use(express.json());

// SIGTERM handler for graceful shutdown
process.on('SIGTERM', async () => {
    console.info('[express] SIGTERM received');
    console.info('[express] cleaning up');
    await new Promise(resolve => setTimeout(resolve, 100));
    console.info('[express] exiting');
    process.exit(0);
});

// Routes
app.get('/', (req, res) => {
    res.json({
        message: 'Hello from Express.js on Azure Functions!',
        framework: 'Express.js',
        adapter: 'Azure Functions Web Adapter',
        timestamp: new Date().toISOString(),
    });
});

app.get('/api/hello', (req, res) => {
    const name = req.query.name || 'World';
    res.json({ message: `Hello, ${name}!` });
});

app.post('/api/echo', (req, res) => {
    res.json({
        received: req.body,
        headers: req.headers,
    });
});

app.get('/api/health', (req, res) => {
    res.json({ status: 'healthy' });
});

app.listen(port, () => {
    console.log(`Express.js app listening at http://localhost:${port}`);
});
