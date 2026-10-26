package org.encryptassets.video;

import android.app.*;
import android.content.*;
import android.content.pm.ActivityInfo;
import android.content.pm.ApplicationInfo;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.*;
import android.os.storage.StorageManager;
import android.security.keystore.*;
import android.system.ErrnoException;
import android.system.OsConstants;
import android.util.Base64;
import android.util.Log;
import android.view.*;
import android.widget.*;
import org.json.*;
import org.videolan.libvlc.*;
import org.videolan.libvlc.interfaces.IVLCVout;
import org.videolan.libvlc.util.VLCVideoLayout;
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.security.*;
import java.security.spec.MGF1ParameterSpec;
import java.util.*;
import java.util.concurrent.*;
import javax.crypto.*;
import javax.crypto.spec.*;

public class MainActivity extends Activity {
    static final String TAG="EAVideo",DEVICE_ALIAS="eav-device-rsa-v1",STATE_ALIAS="eav-state-aes-v1";
    final Handler ui=new Handler(Looper.getMainLooper());
    final ExecutorService worker=Executors.newSingleThreadExecutor();
    HandlerThread ioThread; SharedPreferences prefs; KeyStore keys;
    File selected; JSONObject header; TextView status,contentLabel,timer; Button playButton,pauseButton;
    FrameLayout screen; LinearLayout controls; VLCVideoLayout videoLayout; SeekBar seek;
    LibVLC vlc; MediaPlayer player; ParcelFileDescriptor proxy;
    long handle,expires,runDeadline,lastWall,lastProgress,lastVideoTime,lastTouch; int loops,recoveries; boolean dragging,hidden,playing,paused,busy,foreground,ending; String pendingRequest;
    String error="";
    boolean debug(){return (getApplicationInfo().flags&ApplicationInfo.FLAG_DEBUGGABLE)!=0;}
    int dp(float n){return Math.round(n*getResources().getDisplayMetrics().density);}
    JSONObject obj(Object...kv)throws JSONException{JSONObject j=new JSONObject();for(int i=0;i<kv.length;i+=2)j.put((String)kv[i],kv[i+1]);return j;}
    long epoch(){return System.currentTimeMillis()/1000;}
    File testDir(){File f=new File(getFilesDir(),"test");f.mkdirs();return f;}
    @Override public void onCreate(Bundle b){
        super.onCreate(b);getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);setVolumeControlStream(android.media.AudioManager.STREAM_MUSIC);
        if(!debug())getWindow().addFlags(WindowManager.LayoutParams.FLAG_SECURE);
        prefs=getSharedPreferences("eav",MODE_PRIVATE);ioThread=new HandlerThread("encrypted-video-reader");ioThread.start();
        try{keys=KeyStore.getInstance("AndroidKeyStore");keys.load(null);}catch(Exception e){fail(e);return;}
        String path=prefs.getString("selected",null);if(path!=null){try{selected=new File(path);header=Core.invoke("inspect",obj("path",path));}catch(Exception e){selected=null;header=null;}}
        home();processTest(getIntent());
    }
    @Override protected void onNewIntent(Intent i){super.onNewIntent(i);setIntent(i);processTest(i);}
    void initKeys()throws Exception{
        if(!keys.containsAlias(DEVICE_ALIAS)){KeyPairGenerator g=KeyPairGenerator.getInstance("RSA","AndroidKeyStore");g.initialize(new KeyGenParameterSpec.Builder(DEVICE_ALIAS,KeyProperties.PURPOSE_DECRYPT).setKeySize(2048).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_RSA_OAEP).setDigests(KeyProperties.DIGEST_SHA256,KeyProperties.DIGEST_SHA1).build());g.generateKeyPair();}
        if(!keys.containsAlias(STATE_ALIAS)){KeyGenerator g=KeyGenerator.getInstance("AES","AndroidKeyStore");g.init(new KeyGenParameterSpec.Builder(STATE_ALIAS,KeyProperties.PURPOSE_ENCRYPT|KeyProperties.PURPOSE_DECRYPT).setKeySize(256).setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).build());g.generateKey();}
    }
    String b64(byte[] b){return Base64.encodeToString(b,Base64.NO_WRAP);}
    byte[] un64(String s){return Base64.decode(s,Base64.NO_WRAP);}
    String deviceId()throws Exception{initKeys();byte[] d=MessageDigest.getInstance("SHA-256").digest(keys.getCertificate(DEVICE_ALIAS).getPublicKey().getEncoded());StringBuilder s=new StringBuilder();for(byte b:d)s.append(String.format(Locale.US,"%02x",b));return s.toString();}
    JSONObject state(String id)throws Exception{
        String raw=prefs.getString("state."+id,null);if(raw==null)return new JSONObject();
        JSONObject box=new JSONObject(raw);Cipher c=Cipher.getInstance("AES/GCM/NoPadding");c.init(Cipher.DECRYPT_MODE,keys.getKey(STATE_ALIAS,null),new GCMParameterSpec(128,un64(box.getString("iv"))));c.updateAAD(id.getBytes(StandardCharsets.UTF_8));return new JSONObject(new String(c.doFinal(un64(box.getString("data"))),StandardCharsets.UTF_8));
    }
    void state(String id,JSONObject value)throws Exception{
        initKeys();Cipher c=Cipher.getInstance("AES/GCM/NoPadding");c.init(Cipher.ENCRYPT_MODE,keys.getKey(STATE_ALIAS,null));c.updateAAD(id.getBytes(StandardCharsets.UTF_8));String box=obj("iv",b64(c.getIV()),"data",b64(c.doFinal(value.toString().getBytes(StandardCharsets.UTF_8)))).toString();if(!prefs.edit().putString("state."+id,box).commit())throw new IOException("无法保存授权状态");
    }
    GradientDrawable bg(int color,int radius){GradientDrawable b=new GradientDrawable();b.setColor(color);b.setCornerRadius(dp(radius));return b;}
    TextView text(String s,int size,int color){TextView t=new TextView(this);t.setText(s);t.setTextSize(size);t.setTextColor(color);t.setPadding(0,dp(6),0,dp(6));return t;}
    Button button(String label,Runnable action){Button b=new Button(this);b.setText(label);b.setTextSize(16);b.setTextColor(0xff39234f);b.setAllCaps(false);b.setBackground(bg(0xffeee5f8,14));LinearLayout.LayoutParams p=new LinearLayout.LayoutParams(-1,dp(56));p.setMargins(0,dp(7),0,dp(7));b.setLayoutParams(p);b.setOnClickListener(v->{if(!busy)action.run();});return b;}
    void home(){
        playing=false;hidden=false;getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT);getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR);
        ScrollView scroll=new ScrollView(this);scroll.setFillViewport(true);scroll.setBackgroundColor(0xfffaf7ff);LinearLayout box=new LinearLayout(this);box.setOrientation(1);box.setPadding(dp(26),dp(48),dp(26),dp(30));scroll.addView(box);
        ImageView icon=new ImageView(this);icon.setImageResource(R.drawable.app_icon);icon.setScaleType(ImageView.ScaleType.FIT_CENTER);box.addView(icon,new LinearLayout.LayoutParams(-1,dp(120)));
        TextView title=text("Encrypt your digital assets",24,0xff342044);title.setTypeface(null,Typeface.BOLD);box.addView(title);box.addView(text("安卓视频版 · 离线授权",18,0xff80569d));
        contentLabel=text(header==null?"先导入加密视频，再为本机申请授权。":header.optString("name"),16,0xff544660);box.addView(contentLabel);
        box.addView(button("①  导入加密视频 .eav",()->choose(10,"*/*")));
        box.addView(button("②  获取本机设备识别码",()->exportRequest()));
        box.addView(button("③  导入授权文件 .lic",()->choose(11,"*/*")));
        playButton=button("全屏播放 · 单个循环",()->startAuthorized());playButton.setBackground(bg(0xff8764a7,14));playButton.setTextColor(Color.WHITE);box.addView(playButton);
        status=text(header==null?"等待导入视频":"视频已就绪。导入 LIC 后即可播放。",14,0xff75667f);box.addView(status);
        box.addView(text("1 秒无操作收起控制栏 · 轻触画面唤出\nRust 加密核心 / LibVLC 3.7.7\n0.1.0 真机预览 · 不设置开机自启动",12,0xff8c8096));setContentView(scroll);
    }
    void choose(int code,String mime){Intent i=new Intent(Intent.ACTION_OPEN_DOCUMENT).setType(mime).addCategory(Intent.CATEGORY_OPENABLE);startActivityForResult(i,code);}
    void busy(String message,Checked task){if(busy)return;busy=true;if(status!=null)status.setText(message);worker.submit(()->{try{task.run();}catch(Exception e){ui.post(()->fail(e));}finally{ui.post(()->busy=false);}});}
    interface Checked{void run()throws Exception;}
    void fail(Exception e){error=e.getMessage()==null?e.toString():e.getMessage();Log.e(TAG,error,e);if(status!=null)status.setText(error);new AlertDialog.Builder(this).setTitle("暂时无法继续").setMessage(error).setPositiveButton("知道了",null).show();}
    void copy(InputStream in,File to)throws Exception{try(InputStream source=in;OutputStream out=new FileOutputStream(to)){if(source==null)throw new IOException("文件无法读取");byte[] b=new byte[1024*1024];int n;while((n=source.read(b))!=-1)out.write(b,0,n);}}
    void importVideo(InputStream in)throws Exception{
        File tmp=new File(getFilesDir(),"import.part");try{copy(in,tmp);JSONObject h=Core.invoke("inspect",obj("path",tmp.toString()));File lib=new File(getFilesDir(),"library");lib.mkdirs();File dest=new File(lib,h.getString("id")+".eav");if(dest.exists()){if(!dest.delete())throw new IOException("无法替换已导入文件");}if(!tmp.renameTo(dest))throw new IOException("无法保存视频包");selected=dest;header=h;prefs.edit().putString("selected",dest.toString()).commit();}finally{tmp.delete();}
    }
    String request()throws Exception{
        if(header==null)throw new Exception("请先导入加密视频");initKeys();JSONObject d=obj("public_key",b64(keys.getCertificate(DEVICE_ALIAS).getPublicKey().getEncoded()),"model",Build.MODEL,"manufacturer",Build.MANUFACTURER,"android",Build.VERSION.RELEASE);return Core.invoke("enroll",obj("path",selected.toString(),"device",d)).toString(2);
    }
    void exportRequest(){busy("正在生成加密设备识别文件…",()->{pendingRequest=request();ui.post(()->{Intent i=new Intent(Intent.ACTION_CREATE_DOCUMENT).setType("application/json").addCategory(Intent.CATEGORY_OPENABLE).putExtra(Intent.EXTRA_TITLE,"安卓设备识别码.json");startActivityForResult(i,12);});});}
    String readText(InputStream in)throws Exception{try(InputStream source=in;ByteArrayOutputStream out=new ByteArrayOutputStream()){byte[] b=new byte[8192];int n;while((n=source.read(b))!=-1){if(out.size()+n>256*1024)throw new IOException("授权文件过大");out.write(b,0,n);}return out.toString("UTF-8");}}
    @Override protected void onActivityResult(int req,int result,Intent data){super.onActivityResult(req,result,data);if(result!=RESULT_OK||data==null)return;Uri uri=data.getData();if(uri==null)return;
        if(req==10)busy("正在导入并校验视频包…",()->{importVideo(getContentResolver().openInputStream(uri));ui.post(this::home);});
        if(req==11)busy("正在验证授权…",()->{if(header==null)throw new Exception("请先导入视频");String lic=readText(getContentResolver().openInputStream(uri));activate(new JSONObject(lic),true);ui.post(()->{busy=false;startAuthorized();});});
        if(req==12)busy("正在保存设备识别文件…",()->{try(OutputStream out=getContentResolver().openOutputStream(uri)){out.write(pendingRequest.getBytes(StandardCharsets.UTF_8));}ui.post(()->status.setText("设备识别文件已保存，请发给管理员签发 LIC。"));});
    }
    JSONObject activate(JSONObject lic,boolean isImport)throws Exception{
        JSONObject b=Core.invoke("verify",obj("path",selected.toString(),"license",lic,"device_id",deviceId()));String id=b.getString("license_id");JSONObject s=state(id);long t=epoch();
        if(s.has("last")&&t+120<s.getLong("last"))throw new Exception("检测到系统时间回退，请校准时间");
        if(!s.has("activated")){if(!isImport)throw new Exception("尚未激活此授权");if(t<b.getLong("issued")-120)throw new Exception("系统时间早于授权签发时间");if(t>b.getLong("import_deadline"))throw new Exception("已超过首次导入窗口，请重新申请授权");s.put("activated",t);}
        long end=Math.addExact(s.getLong("activated"),b.getLong("duration"));if(t>=end)throw new Exception("授权已到期");s.put("last",Math.max(t,s.optLong("last",t)));s.put("license",lic);state(id,s);state("content."+header.getString("id"),obj("active_license",id));b.put("expires",end);return b;
    }
    void startAuthorized(){if(header==null){fail(new Exception("请先导入加密视频"));return;}busy("正在打开授权视频…",()->{JSONObject content=state("content."+header.getString("id"));String id=content.optString("active_license","");if(id.isEmpty())throw new Exception("请先导出设备识别码，并导入管理员签发的 LIC");JSONObject b=activate(state(id).getJSONObject("license"),false);Cipher rsa=Cipher.getInstance("RSA/ECB/OAEPPadding");rsa.init(Cipher.DECRYPT_MODE,keys.getKey(DEVICE_ALIAS,null),new OAEPParameterSpec("SHA-256","MGF1",MGF1ParameterSpec.SHA1,PSource.PSpecified.DEFAULT));byte[] key=rsa.doFinal(un64(b.getString("wrapped_key")));JSONObject opened=Core.invoke("open",obj("path",selected.toString(),"key",b64(key),"expires",b.getLong("expires")));Arrays.fill(key,(byte)0);handle=opened.getLong("handle");expires=b.getLong("expires");runDeadline=SystemClock.elapsedRealtime()+(expires-epoch())*1000;long size=opened.getLong("size");
        final long streamHandle=handle;proxy=((StorageManager)getSystemService(STORAGE_SERVICE)).openProxyFileDescriptor(ParcelFileDescriptor.MODE_READ_ONLY,new ProxyFileDescriptorCallback(){
            @Override public long onGetSize(){return size;}
            @Override public int onRead(long offset,int count,byte[] data)throws ErrnoException{try{byte[] result=Core.read(streamHandle,offset,count);System.arraycopy(result,0,data,0,result.length);Arrays.fill(result,(byte)0);return result.length;}catch(Exception e){Log.e(TAG,"encrypted read failed",e);throw new ErrnoException("read",OsConstants.EIO);}}
            @Override public void onRelease(){}
        },new Handler(ioThread.getLooper()));ui.post(this::playScreen);
    });}
    void playScreen(){
        try{
            error="";playing=true;paused=false;hidden=false;ending=false;loops=0;recoveries=0;lastWall=epoch();lastProgress=SystemClock.elapsedRealtime();lastVideoTime=-1;getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE);
            getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY|View.SYSTEM_UI_FLAG_FULLSCREEN|View.SYSTEM_UI_FLAG_HIDE_NAVIGATION|View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN|View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION|View.SYSTEM_UI_FLAG_LAYOUT_STABLE);
            screen=new FrameLayout(this);screen.setBackgroundColor(Color.BLACK);videoLayout=new VLCVideoLayout(this);screen.addView(videoLayout,new FrameLayout.LayoutParams(-1,-1,Gravity.CENTER));
            controls=new LinearLayout(this);controls.setGravity(Gravity.CENTER_VERTICAL);controls.setPadding(dp(10),dp(5),dp(10),dp(5));controls.setBackground(bg(0xe62e223c,18));FrameLayout.LayoutParams bar=new FrameLayout.LayoutParams(-1,dp(68),Gravity.BOTTOM|Gravity.CENTER_HORIZONTAL);bar.setMargins(dp(20),0,dp(20),dp(8));screen.addView(controls,bar);
            pauseButton=small("暂停",()->{if(player==null)return;if(paused){player.play();paused=false;lastProgress=SystemClock.elapsedRealtime();}else{player.pause();paused=true;}pauseButton.setText(paused?"播放":"暂停");});
            small("−10s",()->{if(player!=null){player.setTime(Math.max(0,player.getTime()-10000));lastProgress=SystemClock.elapsedRealtime();}});
            seek=new SeekBar(this);seek.setMax(1000);controls.addView(seek,new LinearLayout.LayoutParams(0,dp(48),1));seek.setOnSeekBarChangeListener(new SeekBar.OnSeekBarChangeListener(){public void onStartTrackingTouch(SeekBar s){dragging=true;showControls();}public void onStopTrackingTouch(SeekBar s){if(player!=null){player.setPosition(s.getProgress()/1000f);lastProgress=SystemClock.elapsedRealtime();}dragging=false;showControls();}public void onProgressChanged(SeekBar s,int p,boolean user){}});
            timer=text("0:00",12,Color.WHITE);controls.addView(timer,new LinearLayout.LayoutParams(dp(88),-2));small("关闭",()->stopPlayback(null));setContentView(screen);
            vlc=new LibVLC(this,new ArrayList<>(Arrays.asList("--no-video-title-show","--no-audio-time-stretch","--file-caching=300")));player=new MediaPlayer(vlc);
            player.attachViews(videoLayout,null,false,false);
            Media media=new Media(vlc,proxy.getFileDescriptor());media.setHWDecoderEnabled(true,false);media.addOption(":input-repeat=65535");media.addOption(":file-caching=300");player.setMedia(media);media.release();
            player.setEventListener(event->{if(!playing)return;if(event.type==MediaPlayer.Event.EncounteredError)recover("VLC 无法解码此视频");if(event.type==MediaPlayer.Event.EndReached&&!ending){ending=true;ui.post(()->{if(playing&&player!=null){player.stop();player.play();loops++;lastProgress=SystemClock.elapsedRealtime();ending=false;}});}});
            player.play();Log.i(TAG,"player started");controls.post(this::showControls);ui.post(tick);
        }catch(Exception e){Log.e(TAG,"player startup failed",e);stopPlayback(e.toString());}
    }
    Button small(String s,Runnable r){Button b=new Button(this);b.setText(s);b.setTextColor(Color.WHITE);b.setTextSize(13);b.setAllCaps(false);b.setMinWidth(0);b.setMinimumWidth(0);b.setPadding(dp(4),0,dp(4),0);b.setBackground(bg(0xff584264,10));LinearLayout.LayoutParams p=new LinearLayout.LayoutParams(dp(64),dp(46));p.setMargins(dp(3),0,dp(3),0);controls.addView(b,p);b.setOnClickListener(v->{r.run();showControls();});return b;}
    final Runnable hide=()->{if(!playing||dragging||controls==null)return;hidden=true;controls.animate().translationY(controls.getHeight()+dp(8)-dp(4)).setDuration(220).start();};
    void showControls(){if(!playing||controls==null)return;hidden=false;lastTouch=SystemClock.elapsedRealtime();controls.animate().translationY(0).setDuration(180).start();ui.removeCallbacks(hide);ui.postDelayed(hide,1000);}
    @Override public boolean dispatchTouchEvent(android.view.MotionEvent e){if(playing){showControls();if(e.getActionMasked()==MotionEvent.ACTION_DOWN||e.getActionMasked()==MotionEvent.ACTION_MOVE)ui.removeCallbacks(hide);if(e.getActionMasked()==MotionEvent.ACTION_UP||e.getActionMasked()==MotionEvent.ACTION_CANCEL)ui.postDelayed(hide,1000);}return super.dispatchTouchEvent(e);}
    @Override public boolean dispatchGenericMotionEvent(android.view.MotionEvent e){if(playing&&e.getActionMasked()==MotionEvent.ACTION_HOVER_MOVE)showControls();return super.dispatchGenericMotionEvent(e);}
    String time(long ms){long s=Math.max(ms,0)/1000;return String.format(Locale.US,"%d:%02d",s/60,s%60);}
    final Runnable tick=new Runnable(){public void run(){if(!playing||player==null)return;long t=epoch(),mono=SystemClock.elapsedRealtime();if(t>=expires||mono>=runDeadline||t+120<lastWall){stopPlayback("授权已到期或系统时间异常");return;}lastWall=Math.max(t,lastWall);long pos=player.getTime(),len=player.getLength();if(!dragging&&len>0)seek.setProgress((int)(1000*pos/len));timer.setText(time(pos)+" / "+time(len));if(pos!=lastVideoTime){if(lastVideoTime>2000&&pos+1500<lastVideoTime&&!dragging)loops++;lastVideoTime=pos;lastProgress=mono;}if(!paused&&foreground&&mono-lastProgress>45000){recover("视频播放长时间没有进展");lastProgress=mono;}writeDiagnostic(pos,len);ui.postDelayed(this,500);}};
    void recover(String reason){if(paused||!foreground)return;recoveries++;if(recoveries>2){stopPlayback(reason+"。已停止自动重试，请检查视频。此预览版不会重启手机。");return;}Log.w(TAG,"recover "+recoveries);if(player!=null){player.stop();player.play();lastProgress=SystemClock.elapsedRealtime();}}
    void writeDiagnostic(long pos,long len){if(!debug())return;try{JSONObject d=obj("playing",playing,"paused",paused,"position",pos,"length",len,"controls_hidden",hidden,"translation_y",controls==null?0:controls.getTranslationY(),"loops_or_backward_seeks",loops,"recoveries",recoveries,"expires",expires,"error",error,"rust",true);try(FileOutputStream f=new FileOutputStream(new File(testDir(),"state.json"))){f.write(d.toString().getBytes(StandardCharsets.UTF_8));}}catch(Exception ignored){}}
    void stopPlayback(String message){
        playing=false;ui.removeCallbacks(tick);ui.removeCallbacks(hide);try{if(player!=null){player.setEventListener(null);player.stop();player.detachViews();player.release();}}catch(Exception ignored){}player=null;
        if(vlc!=null){vlc.release();vlc=null;}try{if(proxy!=null)proxy.close();}catch(Exception ignored){}proxy=null;if(handle!=0){try{Core.call("close",obj("handle",handle).toString());}catch(Exception ignored){}handle=0;}
        home();if(message!=null){error=message;status.setText(message);}writeDiagnostic(0,0);
    }
    @Override public void onBackPressed(){if(playing)stopPlayback(null);else super.onBackPressed();}
    @Override protected void onResume(){super.onResume();foreground=true;}
    @Override protected void onPause(){foreground=false;if(playing&&player!=null&&!paused){player.pause();paused=true;pauseButton.setText("播放");}super.onPause();}
    @Override protected void onDestroy(){if(playing)stopPlayback(null);ui.removeCallbacksAndMessages(null);ioThread.quitSafely();worker.shutdown();super.onDestroy();}
    void processTest(Intent i){
        if(!debug()||!i.hasExtra("test_action"))return;String action=i.getStringExtra("test_action");
        if("enroll".equals(action)){busy("测试：导入合成视频…",()->{importVideo(new FileInputStream(new File(testDir(),"sample.eav")));String r=request();try(FileOutputStream f=new FileOutputStream(new File(testDir(),"request.json"))){f.write(r.getBytes(StandardCharsets.UTF_8));}ui.post(()->{home();status.setText("测试设备请求已导出");});});}
        if("activate".equals(action)){busy("测试：验证真实 LIC…",()->{activate(new JSONObject(readText(new FileInputStream(new File(testDir(),"license.lic")))),true);ui.post(()->{busy=false;startAuthorized();});});}
        if("seek".equals(action)&&player!=null){player.setPosition(0.7f);lastProgress=SystemClock.elapsedRealtime();showControls();}
        if("close".equals(action)&&playing)stopPlayback(null);
    }
}


