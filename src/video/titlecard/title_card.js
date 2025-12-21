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
    // Trigger earlier to switch to space-saving layout
    if (textLength > 500 || (codeBlocks > 0 && textLength > 300)) {
        body.classList.add('layout-dense');
    }

    // 2. Auto-scaling logic
    let currentScale = 100;
    const minScale = 20;
    const maxScale = 300; // Allow growing up to 3x base size
    
    function checkOverflow() {
        // Vertical overflow
        if (body.scrollHeight > body.clientHeight || content.scrollHeight > body.clientHeight) {
            return true;
        }
        
        // Horizontal overflow (global)
        if (body.scrollWidth > body.clientWidth) {
            return true;
        }

        // Horizontal overflow (code blocks)
        // Code blocks usually don't wrap, so we must check if they need scrolling.
        // For a static title card, scrolling = cut off content.
        const pres = content.querySelectorAll('pre');
        for (const pre of pres) {
            if (pre.scrollWidth > pre.clientWidth) {
                return true;
            }
        }

        return false;
    }
    
    // Initial check
    if (checkOverflow()) {
        // Shrink mode
        while (checkOverflow() && currentScale > minScale) {
            currentScale -= 5;
            body.style.fontSize = currentScale + '%';
        }
    } else {
        // Grow mode
        // We grow until it overflows, then step back
        while (!checkOverflow() && currentScale < maxScale) {
            currentScale += 5;
            body.style.fontSize = currentScale + '%';
        }
        
        // If we caused an overflow, step back one unit to make it fit again
        if (checkOverflow()) {
            currentScale -= 5;
            body.style.fontSize = currentScale + '%';
        }
    }

    // 3. Code Block Headers
    const pres = content.querySelectorAll('pre');
    pres.forEach(pre => {
        // Determine container (Pandoc's div.sourceCode or we wrap it)
        let container = pre.parentElement;
        if (!container.classList.contains('sourceCode')) {
            container = document.createElement('div');
            container.classList.add('code-window');
            pre.parentNode.insertBefore(container, pre);
            container.appendChild(pre);
        }

        // Determine language from classes
        let lang = 'CODE';
        const classSources = [pre, pre.querySelector('code')].filter(Boolean);
        for (const el of classSources) {
            for (const cls of el.classList) {
                if (cls !== 'sourceCode' && cls !== 'numberSource' && cls.length > 1) {
                    lang = cls.toUpperCase();
                    break;
                }
            }
            if (lang !== 'CODE') break;
        }

        // Create and insert header
        const header = document.createElement('div');
        header.classList.add('code-header');
        header.innerText = lang;
        container.insertBefore(header, pre);
    });
});
