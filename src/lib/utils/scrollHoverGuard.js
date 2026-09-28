const SCROLL_IDLE_MS = 150;

export function scrollHoverGuard(node) {
  let idleTimer = null;

  function handleScroll() {
    if (idleTimer === null) {
      node.classList.add('is-scrolling');
    } else {
      clearTimeout(idleTimer);
    }
    idleTimer = setTimeout(() => {
      idleTimer = null;
      node.classList.remove('is-scrolling');
    }, SCROLL_IDLE_MS);
  }

  node.addEventListener('scroll', handleScroll, { passive: true });

  return {
    destroy() {
      node.removeEventListener('scroll', handleScroll);
      if (idleTimer !== null) clearTimeout(idleTimer);
    }
  };
}
