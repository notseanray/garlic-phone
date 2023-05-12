<script>
	import Counter from './Counter.svelte';
	import welcome from '$lib/images/svelte-welcome.webp';
	import welcome_fallback from '$lib/images/svelte-welcome.png';
    import { onMount } from "svelte";
    const CANVAS_WIDTH = 600;
    let socket;
    let update_flag = false;
    let selected_color = 1;
    const hex2rgb = (hex) => {
        if (!hex) {
            return;
        }
        const r = parseInt(hex.slice(1, 3), 16);
        const g = parseInt(hex.slice(3, 5), 16);
        const b = parseInt(hex.slice(5, 7), 16);

        // return {r, g, b}
        return [r, g, b, 255];
    }
    const rgbString = (rgb) => {
        return "rgba(" + rgb[0] + "," + rgb[1] + "," + rgb[2] + "," + rgb[3] + ")";
    }
    const og_colors = ["#ffffff", "#ff0000", "#ff4000", "#ffaa00"];
    const colors = ["#ffffff", "#ff0000", "#ff4000", "#ffaa00"].map(c => hex2rgb(c));
    async function generateCircleBrush(red, green, blue, alpha, radius) {
        let data = new Array(16*radius*radius).fill(0);

        for (let x = -radius; x < radius; x++) {
          for (let y = -radius; y < radius; y++) {
            let distance = Math.sqrt(x*x + y*y);

            if (distance > radius) {
              // skip all (x,y) coordinates that are outside of the circle
              continue;
            }

            // Figure out the starting index of this pixel in the image data array.
            let rowLength = 2*radius;
            let adjustedX = x + radius; // convert x from [-50, 50] to [0, 100] (the coordinates of the image data array)
            let adjustedY = y + radius; // convert y from [-50, 50] to [0, 100] (the coordinates of the image data array)
            let pixelWidth = 4; // each pixel requires 4 slots in the data array
            let index = (adjustedX + (adjustedY * rowLength)) * pixelWidth;
            data[index] = red;
            data[index+1] = green;
            data[index+2] = blue;
            data[index+3] = alpha;
          }
        }
        // put image data
        return await createImageBitmap(new ImageData(new Uint8ClampedArray(data), radius * 2, radius * 2));
      }
    let send_buffer = [];
    let canvas_raw;
    let canvas_data;
    let ctx;
    let flag = false;
    let prevX = 0;
    let currX = 0;
    let prevY = 0;
    let currY = 0;
    let dot_flag = false;
    let line_width = 6;
    let brush_image;
    let lastScrollTop = 0;
    const redraw = () => {
        if (canvas_raw == undefined || ctx == undefined) {
            return;
        }
        if (canvas_raw.length == 0) {
            return;
        }
        canvas_data = canvas_raw.map(c => colors[parseInt(c)]).flat();
        ctx.putImageData(new ImageData(new Uint8ClampedArray(canvas_data), 600, 480), 0, 0);
    }


    onMount(async () => {
        document.addEventListener("wheel", async (event) => {
            console.log(event.deltaY)
            line_width -= Math.round(event.deltaY / 10);
            brush_image = await generateCircleBrush(255, 0, 0, 255, line_width / 2);
        });
        brush_image = await generateCircleBrush(255, 0, 0, 255, line_width / 2);
        const c = document.querySelector('canvas');
        ctx = c.getContext('2d');
        ctx.imageSmoothingEnabled = false;
        ctx.translate(-0.5, -0.5);
        c.addEventListener("mousemove", function (e) {
            findxy('move', e)
        }, false);
        c.addEventListener("mousedown", function (e) {
            findxy('down', e)
        }, false);
        c.addEventListener("mouseup", function (e) {
            findxy('up', e)
        }, false);
        c.addEventListener("mouseout", function (e) {
            findxy('out', e)
        }, false);
        function distanceBetween (point1_x, point2_x, point1_y, point2_y) {
          return Math.sqrt(Math.pow(point2_x - point1_x, 2) + Math.pow(point2_y - point1_y, 2))
        }
        function angleBetween (point1_x, point2_x, point1_y, point2_y) {
          return Math.atan2(point2_x - point1_x, point2_y - point1_y)
        }
        function draw() {
            ctx.beginPath();
            ctx.moveTo(prevX, prevY);
            ctx.lineTo(currX, currY);
            const dist = distanceBetween(prevX, currX, prevY, currY);
            const angle = angleBetween(prevX, currX, prevY, currY);
            for (let i = 0; i < dist; i += 3) {
              const x = prevX + (Math.sin(angle) * i);
              const y = prevY + (Math.cos(angle) * i);
              send_buffer.push(parseInt(x));
              send_buffer.push(parseInt(y));
              ctx.drawImage(brush_image, Math.round(x - line_width / 2), Math.round(y - line_width / 2))
              // const r = line_width / 2;
              //   for (let x_c = -r; x_c < r; x_c++) {
              //     for (let y_c = -r; y_c < r; y_c++) {
              //       let distance = Math.sqrt(x_c*x_c + y_c*y_c);
              //
              //       if (distance > r) {
              //         // skip all (x,y) coordinates that are outside of the circle
              //         continue;
              //       }
              //         // send_buffer.push(x + x_c);
              //         // send_buffer.push(y + y_c);
              //     }
              // }
            }
            const c = colors[selected_color];
            ctx.strokeStyle = rgbString(c);
            ctx.lineWidth = line_width;
            ctx.stroke();
            ctx.closePath();
        }
        function findxy(res, e) {
            if (res == 'down') {
                prevX = currX;
                prevY = currY;
                currX = e.clientX - c.offsetLeft;
                currY = e.clientY - c.offsetTop;

                flag = true;
                dot_flag = true;
                if (dot_flag) {
                    ctx.beginPath();
                    ctx.fillStyle = rgbString(colors[selected_color]);
                    ctx.fillRect(currX, currY, 2, 2);
                    ctx.closePath();
                    dot_flag = false;
                }
            }
            if (res == 'up' || res == "out") {
                flag = false;
            }
            if (res == 'move') {
                if (flag) {
                    prevX = currX;
                    prevY = currY;
                    currX = e.clientX - c.offsetLeft;
                    currY = e.clientY - c.offsetTop;
                    draw();
                }
            }
        }
        socket = new WebSocket("ws://127.0.0.1:8069/garlic");
        socket.onmessage = function (event) {
            if (event.data.length > 10000) {
                canvas_raw = event.data.split(" ").map(c => parseInt(c));
                redraw();
            }
        }
        setInterval(async () => {
            if (send_buffer.length >= 2) {
                await socket.send(send_buffer.join(" ") + " " + line_width);
                send_buffer.length = 0;
            }
        }, 200);
        socket.onopen = function (event) {
            // socket.send("test");
        }
    });
</script>

<svelte:head>
	<title>Home</title>
	<meta name="description" content="Svelte demo app" />
</svelte:head>

<section>
<canvas width="600" height="480">
game
</canvas>
{#each og_colors as c, i}
    <button style={"background-color: " + c} on:click={() => {
            console.log(c)
            selected_color = i;
        }}>
    </button>
{/each}
</section>

<style>
    button {
        height: 40px;
        width: 40px;
        border: none;
        margin-left: 4px;
        margin-right: 4px;
    }
    canvas {
        border: 1px solid black;
        image-rendering: optimizeSpeed;
        image-rendering: -moz-crisp-edges;
        image-rendering: -webkit-optimize-contrast;
        image-rendering: -o-crisp-edges;
        image-rendering: optimize-contrast;
        -ms-interpolation-mode: nearest-neighbor;
    }
</style>
