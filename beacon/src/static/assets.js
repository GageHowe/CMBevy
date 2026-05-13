async function uploadSelectedFile() {
    const input = document.getElementById("asset-file");
    const status = document.getElementById("upload-status");
    const result = document.getElementById("upload-result");
    const button = document.getElementById("upload-button");
    const file = input.files[0];
    if (!file) {
        status.textContent = "Choose a file first.";
        return;
    }

    button.disabled = true;
    status.textContent = `Uploading ${file.name}...`;
    result.hidden = true;

    try {
        const response = await fetch("/assets", {
            method: "POST",
            headers: { "x-asset-filename": file.name },
            body: await file.arrayBuffer(),
        });
        if (!response.ok) {
            throw new Error(`Upload failed with ${response.status}`);
        }
        const payload = await response.json();
        document.getElementById("asset-hash").value = payload.hash;
        status.textContent = `${payload.file_name} uploaded successfully.`;
        result.hidden = false;
        reloadAssets();
    } catch (error) {
        status.textContent = error.message;
    } finally {
        button.disabled = false;
    }
}

function updateSelectedFile() {
    const file = document.getElementById("asset-file").files[0];
    document.getElementById("selected-file").textContent = file
        ? file.name
        : "No file selected.";
}

async function voteAsset(hash, vote) {
    await fetch(`/assets/${hash}/vote/${vote}`, { method: "POST" });
    reloadAssets();
}

async function copyAssetHash(hash) {
    await navigator.clipboard.writeText(hash);
}

function reloadAssets() {
    htmx.ajax(
        "GET",
        "/assets/list?q=" +
            encodeURIComponent(document.getElementById("asset-filter").value),
        "#asset-list",
    );
}
