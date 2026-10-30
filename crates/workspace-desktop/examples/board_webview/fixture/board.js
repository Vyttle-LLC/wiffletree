probe.served = true;
setInterval(() => {
  document.getElementById("results").textContent = JSON.stringify(probe, null, 2);
}, 250);
