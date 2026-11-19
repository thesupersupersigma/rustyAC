// Task W3: the render scale changed mid-drive (the graphics page's Render scale), then 3 s more
const before = (await window.rustyac.graphics()).SCALE;
await window.rustyac.graphics({ SCALE: '50' });
await new Promise((resolve) => setTimeout(resolve, 3000));
const canvas = document.getElementById('view');
return { before, after: (await window.rustyac.graphics()).SCALE, canvas: [canvas.width, canvas.height] };
