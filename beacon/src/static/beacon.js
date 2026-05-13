function filterLobbies(query) {
    const cards = document.querySelectorAll(".lobby-card");
    cards.forEach((card) => {
        const name = card.dataset.name.toLowerCase();
        card.style.display = name.includes(query.toLowerCase()) ? "" : "none";
    });
}
