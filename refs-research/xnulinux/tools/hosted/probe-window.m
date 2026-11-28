// SPDX-License-Identifier: GPL-2.0-only
// A rendering probe for the hosted macOS: one borderless window above everything (screen-saver level), for SECONDS seconds.
// MODE picks who renders its content, to tell the client's rendering from WindowServer's compositing apart:
//   draw   AppKit drawRect: four colour quadrants and a label (the client renders its layer tree)
//   image  a layer whose contents is a CGImage of the same quadrants (WindowServer gets the pixels as shared memory)
//   color  a layer with only a red background colour (WindowServer fills it itself; no client pixels at all)
//   clang -fobjc-arc -framework AppKit -framework QuartzCore probe-window.m -o probe-window && ./probe-window [SECONDS] [MODE]
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>

@interface ProbeView : NSView
@end
@implementation ProbeView
- (void)drawRect:(NSRect)dirty {
    NSRect b = self.bounds;
    CGFloat w = b.size.width / 2, h = b.size.height / 2;
    NSColor *colors[4] = {NSColor.systemRedColor, NSColor.systemGreenColor, NSColor.systemBlueColor, NSColor.systemYellowColor};
    for (int i = 0; i < 4; i++) {
        [colors[i] setFill];
        NSRectFill(NSMakeRect((i % 2) * w, (i / 2) * h, w, h));
    }
    NSDictionary *attrs = @{NSFontAttributeName: [NSFont boldSystemFontOfSize:36], NSForegroundColorAttributeName: NSColor.whiteColor};
    [@"xnulinux" drawAtPoint:NSMakePoint(w - 80, h - 20) withAttributes:attrs];
}
@end

int main(int argc, char **argv) {
    @autoreleasepool {
        double seconds = argc > 1 ? atof(argv[1]) : 30;
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(340, 200, 600, 400) styleMask:NSWindowStyleMaskBorderless
                                                         backing:NSBackingStoreBuffered defer:NO];
        window.level = NSScreenSaverWindowLevel;
        const char *mode = argc > 2 ? argv[2] : "draw";
        if (!strcmp(mode, "draw")) {
            window.contentView = [[ProbeView alloc] initWithFrame:NSMakeRect(0, 0, 600, 400)];
        } else {
            NSView *view = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, 600, 400)];
            view.wantsLayer = YES;
            if (!strcmp(mode, "color")) {
                view.layer.backgroundColor = CGColorCreateSRGB(1, 0, 0, 1);
            } else {
                CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
                CGContextRef cg = CGBitmapContextCreate(NULL, 600, 400, 8, 0, space, kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little);
                CGFloat rgb[4][3] = {{1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {1, 1, 0}};
                for (int i = 0; i < 4; i++) {
                    CGContextSetRGBFillColor(cg, rgb[i][0], rgb[i][1], rgb[i][2], 1);
                    CGContextFillRect(cg, CGRectMake((i % 2) * 300, (i / 2) * 200, 300, 200));
                }
                CGImageRef image = CGBitmapContextCreateImage(cg);
                view.layer.contents = (__bridge id)image;
                CGImageRelease(image); CGContextRelease(cg); CGColorSpaceRelease(space);
            }
            window.contentView = view;
        }
        [window orderFrontRegardless];
        fprintf(stderr, "probe-window: %s window %ld on screen %s\n", mode, (long)window.windowNumber, window.screen ? "yes" : "no");
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)(seconds * NSEC_PER_SEC)), dispatch_get_main_queue(), ^{ [NSApp terminate:nil]; });
        [NSApp run];
    }
    return 0;
}
