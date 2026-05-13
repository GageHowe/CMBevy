async function checkStatus() {
    try {
        const res = await fetch("/health");
        const dot = document.getElementById("status-dot");
        const txt = document.getElementById("status-text");
        if (res.ok) {
            dot.className = "status-dot online";
            txt.textContent = "Online";
        } else {
            dot.className = "status-dot offline";
            txt.textContent = "Offline";
        }
    } catch {
        document.getElementById("status-dot").className = "status-dot offline";
        document.getElementById("status-text").textContent = "Offline";
    }
}
