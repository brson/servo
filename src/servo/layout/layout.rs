#[doc = "

The layout task. Performs layout on the dom, builds display lists and sends
them to be rendered

"];

import task::*;
import comm::*;
import gfx::geom;
import gfx::geom::*;
import gfx::renderer;
import dom::base::*;
import display_list::*;
import dom::rcu::scope;
import base::tree;

enum msg {
    build,
    exit
}

fn layout(renderer: chan<renderer::msg>) -> chan<msg> {

    spawn_listener::<msg> {|po|

        let r = rand::rng();


        let mut j = 0f;
        loop {

            let s = scope();
            let ndiv = s.new_node(nk_div);
            let bdiv = base::linked_box(ndiv);

            int::range(0, 100) {|i|
                let w = float::sin((j + i as float) / 10f) * 300f + 400f;

                let h = float::sin((1f + j * 2f+ i as float) / 10f) * 20f + 40f;

                let size = size(
                    int_to_au(w as int),
                    int_to_au(h as int)
                );
                let node = s.new_node(nk_img(size));
                tree::add_child(ndiv, node);
                let b = base::linked_box(node);
                tree::add_child(bdiv, b);
            }

            j += 0.1f;

            alt recv(po) {
              build {
                #debug("layout: received layout request");
                base::reflow_block(bdiv, int_to_au(800));
                let dlist = build_display_list(bdiv);

                send(renderer, gfx::renderer::render(dlist));
              }
              exit {
                break;
              }
            }
        }
    }

}

fn build_display_list(box: @base::box) -> display_list::display_list {
    let mut list = [box_to_display_item(box)];

    for tree::each_child(box) {|c|
        list += build_display_list(c);
    }

    #debug("display_list: %?", list);
    ret list;
}

fn box_to_display_item(box: @base::box) -> display_item {
    let r = rand::rng();
    let item = display_item({
        item_type: solid_color(r.next() as u8, r.next() as u8, r.next() as u8),
        bounds: box.bounds
    });
    #debug("layout: display item: %?", item);
    ret item;
}
