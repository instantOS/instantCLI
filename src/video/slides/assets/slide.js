window.addEventListener('load', () => {
    const body = document.body;
    const content = document.querySelector('.content');

    if (!content) return;

    // 1. Classification
    const counts = {
        headings: content.querySelectorAll('h1, h2, h3').length,
        paragraphs: content.querySelectorAll('p').length,
        codeBlocks: content.querySelectorAll('pre').length,
        listItems: content.querySelectorAll('li').length,
        blockquotes: content.querySelectorAll('blockquote').length,
        figures: content.querySelectorAll('figure').length,
        images: content.querySelectorAll('img').length,
    };
    const textLength = content.innerText.length;
    const paragraphsOutsideBlockquote = content.querySelectorAll('p:not(blockquote p)').length;

    // Helper to check if slide has only specific content types
    const hasOnly = (types) => types.every(type => counts[type] === 0);

    // "Quote" condition: Slide is ONLY a blockquote
    if (counts.blockquotes > 0 && paragraphsOutsideBlockquote === 0 && hasOnly(['headings', 'codeBlocks', 'listItems', 'images', 'figures'])) {
        body.classList.add('layout-quote');
    }

    // "Image" condition: Single image/figure only
    const isSingleFigure = counts.figures === 1 && hasOnly(['headings', 'paragraphs', 'codeBlocks', 'blockquotes', 'listItems']);
    const isSingleImage = counts.images === 1 && counts.figures === 0 && hasOnly(['headings', 'codeBlocks', 'blockquotes', 'listItems']);
    if (isSingleFigure || isSingleImage) {
        body.classList.add('layout-image');
    }

    // "Title" condition: ONLY a single heading
    const isSingleHeading = counts.headings === 1 && hasOnly(['paragraphs', 'codeBlocks', 'listItems', 'blockquotes', 'images', 'figures']);
    if (isSingleHeading) {
        body.classList.add('layout-title');
    }

    // "Hero" condition: Only headers or very minimal text, but NO blockquotes
    if (!isSingleHeading && counts.codeBlocks === 0 && counts.listItems === 0 && counts.blockquotes === 0 && counts.paragraphs <= 1 && counts.headings > 0 && textLength < 200) {
        body.classList.add('layout-hero');
    }

    // "Dense" condition: Lots of text or code
    if (textLength > 500 || (counts.codeBlocks > 0 && textLength > 300)) {
        body.classList.add('layout-dense');
    }

    // 2. Auto-scaling logic
    let currentScale = 100;
    let minScale = 10;
    let maxScale = 300;

    if (body.classList.contains('layout-title')) {
        maxScale = 400;
    } else if (body.classList.contains('layout-hero')) {
        maxScale = 250;
    }

    if (counts.codeBlocks > 0) {
        minScale = 3;
        currentScale = 80;
        body.style.fontSize = currentScale + '%';
    }

    // Cache padding calculations
    const bodyStyle = window.getComputedStyle(body);
    const paddingVertical = parseFloat(bodyStyle.paddingTop) + parseFloat(bodyStyle.paddingBottom);
    const paddingHorizontal = parseFloat(bodyStyle.paddingLeft) + parseFloat(bodyStyle.paddingRight);

    function checkOverflow() {
        const buffer = counts.codeBlocks > 0 ? 10 : 40;
        const availableHeight = window.innerHeight - paddingVertical - buffer;
        const availableWidth = window.innerWidth - paddingHorizontal - buffer;

        return content.scrollHeight > availableHeight || content.scrollWidth > availableWidth;
    }

    function checkWordBreaking() {
        const headings = content.querySelectorAll('h1, h2, h3');
        for (const heading of headings) {
            const headingWidth = heading.getBoundingClientRect().width;
            const textNodes = [];

            // Get all text nodes in the heading
            const walker = document.createTreeWalker(heading, 4, null); // 4 = NodeFilter.SHOW_TEXT
            let node;
            while (node = walker.nextNode()) {
                if (node.textContent.trim()) {
                    textNodes.push(node);
                }
            }

            for (const textNode of textNodes) {
                const words = textNode.textContent.trim().split(/\s+/);
                const range = document.createRange();

                for (const word of words) {
                    if (word.length <= 1) continue;

                    const wordIndex = textNode.textContent.indexOf(word);
                    if (wordIndex === -1) continue;

                    range.setStart(textNode, wordIndex);
                    range.setEnd(textNode, wordIndex + word.length);

                    // If word is wider than 80% of heading width, it's likely breaking
                    if (range.getBoundingClientRect().width > headingWidth * 0.8) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    // Scale adjustment
    const stepSize = counts.codeBlocks > 0 ? 2 : 5;
    let overflow, wordBreaking;

    if (checkOverflow()) {
        // Shrink mode
        while (checkOverflow() && currentScale > minScale) {
            currentScale -= stepSize;
            body.style.fontSize = currentScale + '%';
        }
    } else {
        // Grow mode - cache results to avoid redundant calls
        while (currentScale < maxScale) {
            overflow = checkOverflow();
            wordBreaking = checkWordBreaking();
            if (overflow || wordBreaking) break;

            currentScale += stepSize;
            body.style.fontSize = currentScale + '%';
        }

        // Step back if we caused overflow or word breaking
        if (overflow || wordBreaking) {
            currentScale -= stepSize;
            body.style.fontSize = currentScale + '%';
        }
    }

    // 3. Conditionally reduce padding for code blocks with long lines
    const pres = content.querySelectorAll('pre');
    pres.forEach(pre => {
        const codeElement = pre.querySelector('code');
        if (!codeElement) return;

        // Check if any line would overflow with normal padding
        const codeText = codeElement.textContent;
        const lines = codeText.split('\n');
        const containerWidth = pre.clientWidth;

        // Create a temporary span to measure text width
        const tempSpan = document.createElement('span');
        tempSpan.style.cssText = 'position: absolute; visibility: hidden; white-space: pre; font-family: inherit; font-size: inherit;';
        tempSpan.textContent = lines.reduce((a, b) => a.length > b.length ? a : b);
        document.body.appendChild(tempSpan);

        const longestLineWidth = tempSpan.getBoundingClientRect().width;
        document.body.removeChild(tempSpan);

        // If the longest line needs more than ~75% of container width, reduce padding
        if (longestLineWidth > containerWidth * 0.75) {
            pre.classList.add('code-compact');
        }
        // If it's really long (>90%), use extra compact padding
        if (longestLineWidth > containerWidth * 0.90) {
            pre.classList.add('code-compact-extra');
        }
    });

    // 4. Code Block Headers
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
