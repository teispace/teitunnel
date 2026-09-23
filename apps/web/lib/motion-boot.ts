/**
 * Runs before the first paint: turns motion on unless the reader asked for less, and
 * reveals everything anyway if <Motion> never starts (a script error must not hide content).
 */
export const motionBootScript = `(()=>{try{if(matchMedia("(prefers-reduced-motion: reduce)").matches)return;var d=document.documentElement;d.classList.add("tt-motion");setTimeout(function(){if(!window.__ttReveal)d.classList.remove("tt-motion")},4000)}catch(e){}})()`;
