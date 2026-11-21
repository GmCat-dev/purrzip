# Keep the methods the app screen calls through window.PurrzipAndroid.
-keepclassmembers class io.gmcat.purrzip.MainActivity$Bridge {
    @android.webkit.JavascriptInterface <methods>;
}
