window.addEventListener('load', () => {
    const body = document.body;
    const content = document.querySelector('.content');

    if (!content) return;

    // 1. Classification
    const headings = content.querySelectorAll('h1, h2, h3').length;
    const paragraphs = content.querySelectorAll('p').length;
    const codeBlocks = content.querySelectorAll('pre').length;
    const listItems = content.querySelectorAll('li').length;
    const blockquotes = content.querySelectorAll('blockquote').length;
    const figures = content.querySelectorAll('figure').length;
    const imgElements = content.querySelectorAll('img').length;
    const textLength = content.innerText.length;

    // "Quote" condition: Slide is ONLY a blockquote (no other content outside the blockquote)
    // Note: Pandoc wraps blockquote text in <p>, so we only count <p> tags NOT inside blockquote
    const paragraphsOutsideBlockquote = content.querySelectorAll('p:not(blockquote p)').length;
    if (blockquotes > 0 && paragraphsOutsideBlockquote === 0 && headings === 0 && codeBlocks === 0 && listItems === 0 && imgElements === 0) {
        body.classList.add('layout-quote');
    }

    // "Image" condition: Single image/figure only (Pandoc wraps images in <figure> or <p>)
    const allParagraphsAreImages = paragraphs > 0 && Array.from(content.querySelectorAll('p')).every(p => {
        const childNodes = Array.from(p.childNodes).filter(n => n.nodeType !== 3 || n.textContent.trim() !== '');
        return childNodes.length === 1 && childNodes[0].tagName === 'IMG';
    });
    // Image layout: single figure with image, OR single img in paragraph, with no other content
    const isSingleFigure = figures === 1 && paragraphsOutsideBlockquote === 0 && headings === 0 && codeBlocks === 0 && blockquotes === 0 && listItems === 0;
    const isSingleImageParagraph = imgElements === 1 && figures === 0 && headings === 0 && codeBlocks === 0 && blockquotes === 0 && listItems === 0 && allParagraphsAreImages;
    if (isSingleFigure || isSingleImageParagraph) {
        body.classList.add('layout-image');
    }

    // "Title" condition: ONLY a single heading, nothing else
    const isSingleHeading = headings === 1 && paragraphs === 0 && codeBlocks === 0 && listItems === 0 && blockquotes === 0 && imgElements === 0;
    if (isSingleHeading) {
        body.classList.add('layout-title');
    }

    // "Hero" condition: Only headers or very minimal text, but NO blockquotes
    if (!isSingleHeading && codeBlocks === 0 && listItems === 0 && blockquotes === 0 && paragraphs <= 1 && headings > 0 && textLength < 200) {
        body.classList.add('layout-hero');
    }

    // "Dense" condition: Lots of text or code
    // Trigger earlier to switch to space-saving layout
    if (textLength > 500 || (codeBlocks > 0 && textLength > 300)) {
        body.classList.add('layout-dense');
    }

    // 2. Auto-scaling logic
    let currentScale = 100;
    const minScale = 10;
    let maxScale = 300; // Allow growing up to 3x base size

    if (body.classList.contains('layout-title')) {
        maxScale = 400; // Allow single headings to grow even more
    } else if (body.classList.contains('layout-hero')) {
        maxScale = 250; // Cap hero slightly more to avoid clipping
    }

    function checkOverflow() {
        const buffer = 40; // px safety margin
        
        // Use window dimensions and compare against content's scroll dimensions
        // This is more reliable than checking body.scrollHeight which is fixed to 100vh
        const style = window.getComputedStyle(body);
        const paddingTop = parseFloat(style.paddingTop);
        const paddingBottom = parseFloat(style.paddingBottom);
        const paddingLeft = parseFloat(style.paddingLeft);
        const paddingRight = parseFloat(style.paddingRight);
        
        const availableHeight = window.innerHeight - paddingTop - paddingBottom - buffer;
        const availableWidth = window.innerWidth - paddingLeft - paddingRight - buffer;

        // Vertical overflow
        if (content.scrollHeight > availableHeight) {
            return true;
        }

        // Horizontal overflow
        if (content.scrollWidth > availableWidth) {
            return true;
        }

        // Horizontal overflow (code blocks)
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
