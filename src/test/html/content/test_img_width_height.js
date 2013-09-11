let img = window.document.getElementsByTagName("img")[0];

function wait_for_img_load(img, f) {
	if (img.width != 0) {
		f();
	} else {
		window.setTimeout(function() { wait_for_img_load(img, f) }, 1);
	}
}

wait_for_img_load(img, function() {
	is(img.width, 500);
	is(img.height, 378);
	finish();
});
