window.addEventListener('load', () => {
    const body = document.body;
    const content = document.querySelector('.content');
    
    if (!content) return;

    // 1. Classification
    const headings = content.querySelectorAll('h1, h2, h3').length;
    const paragraphs = content.querySelectorAll('p').length;
    const codeBlocks = content.querySelectorAll('pre').length;
    const listItems = content.querySelectorAll('li').length;
    const textLength = content.innerText.length;

    // "Hero" condition: Only headers or very minimal text
    if (codeBlocks === 0 && listItems === 0 && paragraphs <= 1 && headings > 0 && textLength < 200) {
        body.classList.add('layout-hero');
    }
    
    // "Dense" condition: Lots of text or code
    if (textLength > 1000 || (codeBlocks > 0 && textLength > 600)) {
        body.classList.add('layout-dense');
    }

    // 2. Auto-scaling logic
    let currentScale = 100;
    const minScale = 40;
    
    // Allow layout to settle
    // We check if content height exceeds viewport height
    // body has height: 100vh and overflow: hidden in CSS
    // So we check scrollHeight vs clientHeight
    
    while ((body.scrollHeight > body.clientHeight || content.scrollHeight > body.clientHeight) && currentScale > minScale) {
        currentScale -= 2;
        body.style.fontSize = currentScale + '%';
    }
});
