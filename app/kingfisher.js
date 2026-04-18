/* =============================================================
   Halcyon — Kingfisher mark
   Original geometric silhouette. The shape references the bird
   diving: a wedge, a wing-sweep, a beak-line. Not photographic.
   Used in the brand lockup and in empty states.
   ============================================================= */
(function(){

  // SVG paths — built from a single compound path in multiple sizes.
  // The mark is a stylised kingfisher in profile, diving downward,
  // composed of: head+eye, dagger beak, folded wing, tail wedge.
  //
  // 24x24 base grid. All sizes share the same geometry.

  // Profile of a kingfisher mid-dive: long dagger beak leading,
  // streamlined head, swept-back wing, short tail wedge. Body axis
  // tilts about 25deg downward from horizontal (diving line).
  //
  // 24x24 grid. Beak tip at (22.2, 7.4). Tail at (2.4, 13.6).
  const MARK_PATH = `
    M 22.2 7.4
    L 13.8 8.5
    L 11.4 8.2
    C 9.0 8.1, 6.9 9.0, 5.3 10.6
    L 8.2 10.7
    L 5.8 12.2
    L 3.6 11.7
    L 2.4 13.6
    L 5.4 13.1
    L 7.0 14.6
    L 9.4 13.7
    L 12.2 14.0
    C 14.0 13.6, 15.0 12.4, 15.0 10.8
    L 17.0 10.5
    L 15.8 9.6
    Z
  `;

  // eye — small negative circle
  const EYE = { cx: 13.4, cy: 9.6, r: 0.42 };

  function renderMark(size, { color='currentColor', title='Halcyon', eye=true } = {}) {
    const svgNS = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(svgNS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('width', size);
    svg.setAttribute('height', size);
    svg.setAttribute('role', 'img');
    svg.setAttribute('aria-label', title);
    svg.style.display = 'inline-block';
    svg.style.verticalAlign = 'middle';
    svg.style.flex = 'none';

    const g = document.createElementNS(svgNS, 'g');
    g.setAttribute('fill', color);

    const p = document.createElementNS(svgNS, 'path');
    p.setAttribute('d', MARK_PATH.replace(/\s+/g, ' ').trim());
    g.appendChild(p);

    if (eye && size >= 16) {
      // knock out the eye
      const circle = document.createElementNS(svgNS, 'circle');
      circle.setAttribute('cx', EYE.cx);
      circle.setAttribute('cy', EYE.cy);
      circle.setAttribute('r', EYE.r);
      circle.setAttribute('fill', 'var(--paper, #FAFAF7)');
      g.appendChild(circle);
    }

    svg.appendChild(g);
    return svg;
  }

  // static string version for embedding in HTML strings
  function markHTML(size, color='currentColor') {
    const d = MARK_PATH.replace(/\s+/g, ' ').trim();
    const eye = size >= 16
      ? `<circle cx="${EYE.cx}" cy="${EYE.cy}" r="${EYE.r}" fill="var(--paper, #FAFAF7)"/>`
      : '';
    return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" role="img" aria-label="Halcyon" style="display:inline-block;vertical-align:middle;flex:none"><g fill="${color}"><path d="${d}"/>${eye}</g></svg>`;
  }

  // Editorial variant — larger, with a diagonal "dive line" below
  function editorialMarkHTML(w=240, h=240, color='currentColor') {
    const d = MARK_PATH.replace(/\s+/g, ' ').trim();
    return `
      <svg viewBox="0 0 240 240" width="${w}" height="${h}" role="img" aria-label="Halcyon editorial">
        <g transform="translate(48, 28) scale(6)" fill="${color}">
          <path d="${d}"/>
          <circle cx="${EYE.cx}" cy="${EYE.cy}" r="${EYE.r}" fill="var(--paper, #FAFAF7)"/>
        </g>
        <line x1="60" y1="210" x2="200" y2="210" stroke="${color}" stroke-width="1" opacity="0.25"/>
        <line x1="90" y1="218" x2="170" y2="218" stroke="${color}" stroke-width="1" opacity="0.15"/>
      </svg>
    `;
  }

  window.Kingfisher = { renderMark, markHTML, editorialMarkHTML, MARK_PATH, EYE };

  // Auto-hydrate <span data-king="16"> etc.
  document.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('[data-king]').forEach(el => {
      const size = parseInt(el.getAttribute('data-king'), 10) || 24;
      const color = el.getAttribute('data-king-color') || 'currentColor';
      el.innerHTML = markHTML(size, color);
    });
  });
})();
