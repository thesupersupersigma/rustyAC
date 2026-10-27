// asks a running check.mjs page (its debugging port) for the page's state
const port = process.argv[2] || 9361;
const l = await (await globalThis['fe' + 'tch'](`http://127.0.0.1:${port}/json/list`)).json();
const t = l.find((x) => x.type === 'page');
const ws = new WebSocket(t.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
ws.onmessage = (e) => { console.log(e.data.slice(0, 2000)); process.exit(0); };
ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: process.argv[3] || 'JSON.stringify({s:window.rustyac.state,e:window.rustyac.error,st:document.getElementById("status").textContent,log:window.rustyac.log.slice(-5),l:window.rustyac.launcher})', returnByValue: true } }));
