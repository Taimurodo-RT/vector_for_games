document.querySelector('#theme').addEventListener('change',e=>document.body.dataset.theme=e.target.value);
document.querySelector('#surface').addEventListener('change',e=>document.body.dataset.surface=e.target.value);
document.querySelectorAll('.motion').forEach(s=>s.addEventListener('change',()=>s.closest('article').dataset.motion=s.value));
